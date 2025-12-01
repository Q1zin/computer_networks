use crate::{
    connection::{Connection, EndpointKind},
    socks5::state::ClientState,
};
use log::{info, warn};
use mio::{Interest, Token};
use std::{collections::HashMap, io::Result};

pub fn update_interests(conn: &mut Connection, registry: &mio::Registry) -> Result<()> {
    let mut client_interest = Interest::READABLE;
    if !conn.target_to_client.is_empty() {
        client_interest = client_interest.add(Interest::WRITABLE);
    }

    registry.reregister(&mut conn.client, conn.client_token, client_interest)?;

    if let (Some(target), Some(token)) = (conn.target.as_mut(), conn.target_token) {
        let mut target_interest = Interest::READABLE;
        if matches!(conn.state, ClientState::Connecting) || !conn.client_to_target.is_empty() {
            target_interest = target_interest.add(Interest::WRITABLE);
        }
        registry.reregister(target, token, target_interest)?;
    }
    Ok(())
}

pub fn cleanup_connection(
    conn_id: usize,
    registry: &mio::Registry,
    connections: &mut HashMap<usize, Connection>,
    token_map: &mut HashMap<Token, (usize, EndpointKind)>,
) -> Result<()> {
    if let Some(mut conn) = connections.remove(&conn_id) {
        let _ = registry.deregister(&mut conn.client);
        if let Some(mut target) = conn.target.take() {
            let _ = registry.deregister(&mut target);
        }
        info!(
            "[conn {conn_id}] Disconnected client {} (last requested: {})",
            conn.client_addr,
            conn.requested_endpoint.as_deref().unwrap_or("<none>")
        );
    } else {
        warn!(
            "No connection found for conn_id {} and can't cleanup",
            conn_id
        );
    }

    token_map.retain(|_, (id, _)| *id != conn_id);
    Ok(())
}
