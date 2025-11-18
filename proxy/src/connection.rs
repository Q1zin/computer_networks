use crate::socks5::state::ClientState;
use mio::{net::TcpStream, Token};
use std::net::SocketAddr;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndpointKind {
    Client,
    Target,
}

pub struct Connection {
    pub client: TcpStream,
    pub client_token: Token,
    pub client_addr: SocketAddr,
    pub target: Option<TcpStream>,
    pub target_token: Option<Token>,
    pub state: ClientState,
    pub client_buf: Vec<u8>,
    pub client_to_target: Vec<u8>,
    pub target_to_client: Vec<u8>,
    pub client_closed: bool,
    pub target_closed: bool,
    pub requested_endpoint: Option<String>,
}

impl Connection {
    pub fn new(client: TcpStream, client_token: Token, client_addr: SocketAddr) -> Self {
        Connection {
            client,
            client_token,
            client_addr,
            target: None,
            target_token: None,
            state: ClientState::Handshake,
            client_buf: Vec::new(),
            client_to_target: Vec::new(),
            target_to_client: Vec::new(),
            client_closed: false,
            target_closed: false,
            requested_endpoint: None,
        }
    }

    pub fn should_close(&self) -> bool {
        (self.client_closed && self.target_to_client.is_empty())
            || (self.target_closed && self.client_to_target.is_empty())
    }
}
