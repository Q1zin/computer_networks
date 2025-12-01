use crate::{
    connection::{Connection, EndpointKind},
    dns::{DnsEvent, DnsResolver},
    handler::{handle_readable, handle_writable},
    util::cleanup_connection,
};
use log::{info, warn};
use mio::{net::TcpListener, Events, Interest, Poll, Token};
use std::{
    collections::HashMap,
    io::{ErrorKind, Result},
};

const SERVER: Token = Token(0);
const DNS: Token = Token(1);

pub struct Server {
    listener: TcpListener,
    poll: Poll,
    connections: HashMap<usize, Connection>,
    token_map: HashMap<Token, (usize, EndpointKind)>,
    next_connection_id: usize,
    next_token: usize,
    dns_resolver: DnsResolver,
}

impl Server {
    pub fn new(port: u16) -> Result<Self> {
        let addr = format!("0.0.0.0:{}", port).parse().unwrap();
        let mut listener = TcpListener::bind(addr)?;
        let poll = Poll::new()?;

        poll.registry()
            .register(&mut listener, SERVER, Interest::READABLE)?;

        let dns_socket_addr = "0.0.0.0:0".parse().unwrap();
        let mut dns_socket = mio::net::UdpSocket::bind(dns_socket_addr)?;

        poll.registry()
            .register(&mut dns_socket, DNS, Interest::READABLE)?;

        let dns_resolver = DnsResolver::new(dns_socket)?;

        println!("SOCKS5 proxy listening on {}", addr);

        Ok(Server {
            listener,
            poll,
            connections: HashMap::new(),
            token_map: HashMap::new(),
            next_connection_id: 1,
            next_token: 2,
            dns_resolver,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let mut events = Events::with_capacity(1024);

        loop {
            self.poll.poll(&mut events, None)?;

            for event in events.iter() {
                match event.token() {
                    SERVER => self.accept_connections()?,
                    DNS => self.handle_dns_socket()?,
                    token => self.handle_connection_event(token, event)?,
                }
            }

            for event in self.dns_resolver.cleanup_expired() {
                self.handle_dns_event(event)?;
            }
        }
    }

    fn accept_connections(&mut self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((mut client, client_addr)) => {
                    let client_token = Token(self.next_token);
                    let conn_id = self.next_connection_id;
                    self.next_token += 1;
                    self.next_connection_id += 1;

                    self.poll
                        .registry()
                        .register(&mut client, client_token, Interest::READABLE)?;

                    self.token_map
                        .insert(client_token, (conn_id, EndpointKind::Client));
                    self.connections
                        .insert(conn_id, Connection::new(client, client_token, client_addr));

                    info!(
                        "[conn {conn_id}] New client {client_addr} (token {:?})",
                        client_token
                    );
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn handle_dns_socket(&mut self) -> Result<()> {
        let events = self.dns_resolver.handle_responses()?;
        for event in events {
            self.handle_dns_event(event)?;
        }
        Ok(())
    }

    fn handle_dns_event(&mut self, event: DnsEvent) -> Result<()> {
        match event {
            DnsEvent::Resolved {
                conn_id,
                resolved_addr,
                display,
            } => {
                let mut should_cleanup = false;
                {
                    if let Some(conn) = self.connections.get_mut(&conn_id) {
                        conn.requested_endpoint = Some(display.clone());

                        match mio::net::TcpStream::connect(resolved_addr) {
                            Ok(mut stream) => {
                                let target_token = Token(self.next_token);
                                self.next_token += 1;
                                self.poll.registry().register(
                                    &mut stream,
                                    target_token,
                                    Interest::WRITABLE,
                                )?;
                                self.token_map
                                    .insert(target_token, (conn_id, EndpointKind::Target));
                                conn.target = Some(stream);
                                conn.target_token = Some(target_token);
                                conn.state = crate::socks5::state::ClientState::Connecting;
                                info!("[conn {conn_id}] Connecting to resolved target {}", display);
                            }
                            Err(e) => {
                                warn!("[conn {conn_id}] Failed to connect to {}: {}", display, e);
                                let response = crate::socks5::protocol::create_refused_response();
                                use std::io::Write;
                                let _ = conn.client.write_all(&response);
                                should_cleanup = true;
                            }
                        }
                    }
                }

                if should_cleanup {
                    cleanup_connection(
                        conn_id,
                        self.poll.registry(),
                        &mut self.connections,
                        &mut self.token_map,
                    )?;
                }
            }
            DnsEvent::Failed {
                conn_id,
                domain,
                reason,
            } => {
                let mut should_cleanup = false;
                {
                    if let Some(conn) = self.connections.get_mut(&conn_id) {
                        warn!(
                            "[conn {conn_id}] DNS resolution failed for {}: {}",
                            domain, reason
                        );
                        let response = crate::socks5::protocol::create_refused_response();
                        use std::io::Write;
                        let _ = conn.client.write_all(&response);
                        should_cleanup = true;
                    }
                }

                if should_cleanup {
                    cleanup_connection(
                        conn_id,
                        self.poll.registry(),
                        &mut self.connections,
                        &mut self.token_map,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn handle_connection_event(&mut self, token: Token, event: &mio::event::Event) -> Result<()> {
        if let Some(&(conn_id, endpoint)) = self.token_map.get(&token) {
            if let Some(conn) = self.connections.get_mut(&conn_id) {
                let mut close = false;

                if event.is_readable() {
                    if handle_readable(
                        conn,
                        conn_id,
                        endpoint,
                        self.poll.registry(),
                        &mut self.token_map,
                        &mut self.next_token,
                        &mut self.dns_resolver,
                    )
                    .is_err()
                    {
                        close = true;
                    }
                }

                if event.is_writable() {
                    if handle_writable(conn, endpoint, self.poll.registry()).is_err() {
                        close = true;
                    }
                }

                if conn.should_close() {
                    close = true;
                }

                if close {
                    cleanup_connection(
                        conn_id,
                        self.poll.registry(),
                        &mut self.connections,
                        &mut self.token_map,
                    )?;
                }
            } else {
                warn!("No connection found for conn_id {}", conn_id);
            }
        } else {
            warn!("No connection found for token {:?}", token);
        }
        Ok(())
    }
}
