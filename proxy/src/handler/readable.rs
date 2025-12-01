use crate::{
    connection::{Connection, EndpointKind},
    dns::DnsResolver,
    socks5::{protocol::*, state::ClientState},
};
use log::{error, info, warn};
use mio::{net::TcpStream, Interest, Token};
use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Read, Result, Write},
};

pub fn handle_readable(
    conn: &mut Connection,
    conn_id: usize,
    endpoint: EndpointKind,
    registry: &mio::Registry,
    token_map: &mut HashMap<Token, (usize, EndpointKind)>,
    next_token: &mut usize,
    dns_resolver: &mut DnsResolver,
) -> Result<()> {
    match endpoint {
        EndpointKind::Client => {
            handle_client_readable(conn, conn_id, registry, token_map, next_token, dns_resolver)
        }
        EndpointKind::Target => handle_target_readable(conn, registry),
    }
}

fn handle_client_readable(
    conn: &mut Connection,
    conn_id: usize,
    registry: &mio::Registry,
    token_map: &mut HashMap<Token, (usize, EndpointKind)>,
    next_token: &mut usize,
    dns_resolver: &mut DnsResolver,
) -> Result<()> {
    match conn.state {
        ClientState::Handshake => handle_handshake(conn),
        ClientState::Request => handle_request(conn, conn_id, registry, token_map, next_token, dns_resolver),
        ClientState::Tunneling => handle_client_data(conn, registry),
        ClientState::Connecting => Ok(()),
        ClientState::Resolving => Ok(()),
    }
}

fn handle_handshake(conn: &mut Connection) -> Result<()> {
    let mut buf = [0u8; 257];
    match conn.client.read(&mut buf) {
        Ok(0) => {
            conn.client_closed = true;
            return Ok(());
        }
        Ok(n) => {
            conn.client_buf.extend_from_slice(&buf[..n]);
            if conn.client_buf.len() >= 2 {
                let nmethods = conn.client_buf[1] as usize;
                if conn.client_buf.len() >= 2 + nmethods {
                    let version = conn.client_buf[0];
                    if version != SOCKS_VERSION {
                        return Err(Error::new(ErrorKind::InvalidData, "Invalid version"));
                    }

                    let response = create_auth_response();
                    conn.client.write_all(&response)?;

                    conn.client_buf.clear();
                    conn.state = ClientState::Request;
                } else {
                    warn!("Handshake data incomplete, waiting for more data (but len >= 2)");
                }
            } else {
                warn!("Handshake data incomplete, waiting for more data");
            }
        }
        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

fn handle_request(
    conn: &mut Connection,
    conn_id: usize,
    registry: &mio::Registry,
    token_map: &mut HashMap<Token, (usize, EndpointKind)>,
    next_token: &mut usize,
    dns_resolver: &mut DnsResolver,
) -> Result<()> {
    let mut buf = [0u8; 512];
    match conn.client.read(&mut buf) {
        Ok(0) => {
            conn.client_closed = true;
            return Ok(());
        }
        Ok(n) => {
            conn.client_buf.extend_from_slice(&buf[..n]);
            if let Some(request_info) = parse_request(&conn.client_buf)? {
                match request_info {
                    RequestInfo::Resolved { addr, display } => {
                        info!(
                            "[conn {conn_id}] Client {} requested {}",
                            conn.client_addr, display
                        );
                        conn.requested_endpoint = Some(display.clone());

                        match TcpStream::connect(addr) {
                            Ok(mut stream) => {
                                let target_token = Token(*next_token);
                                *next_token += 1;
                                registry.register(&mut stream, target_token, Interest::WRITABLE)?;
                                token_map.insert(target_token, (conn_id, EndpointKind::Target));
                                conn.target = Some(stream);
                                conn.target_token = Some(target_token);
                                conn.state = ClientState::Connecting;
                                info!("[conn {conn_id}] Connecting to target {}", display);
                                conn.client_buf.clear();
                            }
                            Err(_) => {
                                let response = create_refused_response();
                                conn.client.write_all(&response)?;
                                error!("[conn {conn_id}] Connection to {} refused", display);
                                return Err(Error::new(
                                    ErrorKind::ConnectionRefused,
                                    "Connection refused",
                                ));
                            }
                        }
                    }
                    RequestInfo::NeedsResolution { domain, port } => {
                        info!(
                            "[conn {conn_id}] Client {} requested {}:{} (needs DNS resolution)",
                            conn.client_addr, domain, port
                        );
                        
                        match dns_resolver.resolve(domain, port, conn_id) {
                            Ok(query_id) => {
                                conn.state = ClientState::Resolving;
                                conn.client_buf.clear();
                                info!("[conn {conn_id}] DNS query started (query_id: {})", query_id);
                            }
                            Err(e) => {
                                error!("[conn {conn_id}] Failed to start DNS resolution: {}", e);
                                let response = create_refused_response();
                                conn.client.write_all(&response)?;
                                return Err(e);
                            }
                        }
                    }
                }
            }
        }
        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

fn handle_client_data(conn: &mut Connection, registry: &mio::Registry) -> Result<()> {
    let mut buf = [0u8; BUFFER_SIZE];
    match conn.client.read(&mut buf) {
        Ok(0) => {
            conn.client_closed = true;
            if let Some(ref mut target) = conn.target {
                let _ = target.shutdown(std::net::Shutdown::Write);
            }
        }
        Ok(n) => {
            conn.client_to_target.extend_from_slice(&buf[..n]);
            if let Some(ref mut target) = conn.target {
                if let Some(token) = conn.target_token {
                    registry.reregister(
                        target,
                        token,
                        Interest::READABLE.add(Interest::WRITABLE),
                    )?;
                }
            }
        }
        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

fn handle_target_readable(conn: &mut Connection, registry: &mio::Registry) -> Result<()> {
    if let Some(ref mut target) = conn.target {
        let mut buf = [0u8; BUFFER_SIZE];
        match target.read(&mut buf) {
            Ok(0) => {
                conn.target_closed = true;
                let _ = conn.client.shutdown(std::net::Shutdown::Write);
            }
            Ok(n) => {
                conn.target_to_client.extend_from_slice(&buf[..n]);
                registry.reregister(
                    &mut conn.client,
                    conn.client_token,
                    Interest::READABLE.add(Interest::WRITABLE),
                )?;
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
