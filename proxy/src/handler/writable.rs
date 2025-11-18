use crate::{
    connection::{Connection, EndpointKind},
    socks5::{protocol::create_success_response, state::ClientState},
    util::update_interests,
};
use log::info;
use mio::Interest;
use std::io::{ErrorKind, Result, Write};

pub fn handle_writable(
    conn: &mut Connection,
    endpoint: EndpointKind,
    registry: &mio::Registry,
) -> Result<()> {
    match endpoint {
        EndpointKind::Client => handle_client_writable(conn),
        EndpointKind::Target => handle_target_writable(conn, registry),
    }?;

    update_interests(conn, registry)?;
    Ok(())
}

fn handle_client_writable(conn: &mut Connection) -> Result<()> {
    if matches!(conn.state, ClientState::Tunneling) {
        if !conn.target_to_client.is_empty() {
            match conn.client.write(&conn.target_to_client) {
                Ok(n) => {
                    conn.target_to_client.drain(..n);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

fn handle_target_writable(conn: &mut Connection, registry: &mio::Registry) -> Result<()> {
    match conn.state {
        ClientState::Connecting => {
            let response = create_success_response();
            conn.client.write_all(&response)?;
            conn.state = ClientState::Tunneling;
            if let Some(ref endpoint) = conn.requested_endpoint {
                info!(
                    "[conn {:?}] Tunnel established for {endpoint}",
                    conn.client_token
                );
            }
            registry.reregister(&mut conn.client, conn.client_token, Interest::READABLE)?;
            if let Some(ref mut target) = conn.target {
                if let Some(token) = conn.target_token {
                    registry.reregister(target, token, Interest::READABLE)?;
                }
            }
        }
        ClientState::Tunneling => {
            if let Some(ref mut target) = conn.target {
                if !conn.client_to_target.is_empty() {
                    match target.write(&conn.client_to_target) {
                        Ok(n) => {
                            conn.client_to_target.drain(..n);
                        }
                        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}
