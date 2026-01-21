use crate::snakes::{GameMessage, NodeRole};
use anyhow::Result;
use std::net::SocketAddr;

pub trait NetworkProtocol: Send + Sync {
    fn serialize(&self, msg: &GameMessage) -> Vec<u8>;
    fn deserialize(&self, bytes: &[u8]) -> Result<GameMessage>;

    fn send_unicast(&self, addr: SocketAddr, msg: GameMessage) -> Result<()>;
    fn send_multicast(&self, msg: GameMessage) -> Result<()>;

    fn poll_receive(&self) -> Result<Option<(GameMessage, SocketAddr)>>;

    fn get_local_addr(&self) -> Result<SocketAddr>;
    fn set_role(&self, role: NodeRole);
}
