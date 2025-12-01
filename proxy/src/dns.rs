use log::{error, info, warn};
use mio::net::UdpSocket;
use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Result},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query},
    rr::{Name, RData, RecordType},
    serialize::binary::{BinDecodable, BinEncodable},
};

const DNS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct DnsRequest {
    pub domain: String,
    pub port: u16,
    pub conn_id: usize,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub enum DnsEvent {
    Resolved {
        conn_id: usize,
        resolved_addr: SocketAddr,
        display: String,
    },
    Failed {
        conn_id: usize,
        domain: String,
        reason: String,
    },
}

pub struct DnsResolver {
    socket: UdpSocket,
    resolver_addr: SocketAddr,
    pending_requests: HashMap<u16, DnsRequest>,
    next_query_id: u16,
}

impl DnsResolver {
    pub fn new(socket: UdpSocket) -> Result<Self> {
        let resolver_addr = Self::get_system_resolver()?;
        info!("Using DNS resolver: {}", resolver_addr);

        Ok(DnsResolver {
            socket,
            resolver_addr,
            pending_requests: HashMap::new(),
            next_query_id: 1,
        })
    }

    fn get_system_resolver() -> Result<SocketAddr> {
        #[cfg(unix)]
        {
            if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with("nameserver") {
                        if let Some(ip) = line.split_whitespace().nth(1) {
                            if let Ok(addr) = ip.parse::<IpAddr>() {
                                return Ok(SocketAddr::new(addr, 53));
                            }
                        }
                    }
                }
            }
        }

        Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53))
    }

    pub fn resolve(&mut self, domain: String, port: u16, conn_id: usize) -> Result<u16> {
        let query_id = self.next_query_id;
        self.next_query_id = self.next_query_id.wrapping_add(1);

        info!(
            "[conn {}] Starting DNS query for {} (query_id: {})",
            conn_id, domain, query_id
        );

        let name = match Name::from_utf8(&domain) {
            Ok(n) => n,
            Err(e) => {
                error!("Invalid domain name {}: {}", domain, e);
                return Err(Error::new(ErrorKind::InvalidInput, "Invalid domain name"));
            }
        };

        let mut msg = Message::new();
        msg.set_id(query_id)
            .set_message_type(MessageType::Query)
            .set_op_code(OpCode::Query)
            .set_recursion_desired(true);

        let query = Query::query(name, RecordType::A);
        msg.add_query(query);

        let bytes = match msg.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to encode DNS message: {}", e);
                return Err(Error::new(ErrorKind::Other, "Failed to encode DNS message"));
            }
        };

        match self.socket.send_to(&bytes, self.resolver_addr) {
            Ok(n) => info!("Sent {} bytes to DNS resolver", n),
            Err(e) => {
                error!("Failed to send DNS query: {}", e);
                return Err(e);
            }
        }

        self.pending_requests.insert(
            query_id,
            DnsRequest {
                domain,
                port,
                conn_id,
                timestamp: Instant::now(),
            },
        );

        Ok(query_id)
    }

    pub fn handle_responses(&mut self) -> Result<Vec<DnsEvent>> {
        let mut events = Vec::new();

        loop {
            let mut buf = [0u8; 512];
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if from != self.resolver_addr {
                        info!("Received DNS response from unexpected source: {}", from);
                        continue;
                    }

                    info!("Received {} bytes from DNS resolver", n);

                    let msg = match Message::from_bytes(&buf[..n]) {
                        Ok(m) => m,
                        Err(e) => {
                            error!("Failed to parse DNS response: {}", e);
                            continue;
                        }
                    };

                    let query_id = msg.id();
                    info!("DNS response query_id: {}", query_id);

                    if let Some(request) = self.pending_requests.remove(&query_id) {
                        info!(
                            "[conn {}] Received DNS response for {} (query_id: {})",
                            request.conn_id, request.domain, query_id
                        );

                        let mut resolved = None;
                        for answer in msg.answers() {
                            if let &RData::A(a_record) = answer.data() {
                                resolved = Some(a_record.0);
                                break;
                            }
                        }

                        if let Some(ipv4) = resolved {
                            let socket_addr = SocketAddr::new(IpAddr::V4(ipv4), request.port);
                            let display = format!("{}:{}", request.domain, request.port);
                            info!(
                                "[conn {}] Resolved {} to {}",
                                request.conn_id, request.domain, ipv4
                            );
                            events.push(DnsEvent::Resolved {
                                conn_id: request.conn_id,
                                resolved_addr: socket_addr,
                                display,
                            });
                        } else {
                            warn!(
                                "[conn {}] No A record found in DNS response for {}",
                                request.conn_id, request.domain
                            );
                            events.push(DnsEvent::Failed {
                                conn_id: request.conn_id,
                                domain: request.domain,
                                reason: "No A record in response".to_string(),
                            });
                        }
                    } else {
                        info!("Received DNS response for unknown query_id: {}", query_id);
                    }
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    error!("Error reading from DNS socket: {}", e);
                    return Err(e);
                }
            }
        }

        Ok(events)
    }

    pub fn cleanup_expired(&mut self) -> Vec<DnsEvent> {
        let now = Instant::now();
        let mut expired = Vec::new();
        self.pending_requests.retain(|_, req| {
            if now.duration_since(req.timestamp) >= DNS_TIMEOUT {
                warn!(
                    "[conn {}] DNS query for {} timed out",
                    req.conn_id, req.domain
                );
                expired.push(DnsEvent::Failed {
                    conn_id: req.conn_id,
                    domain: req.domain.clone(),
                    reason: "DNS query timed out".to_string(),
                });
                false
            } else {
                true
            }
        });
        expired
    }
}
