#[derive(Debug)]
pub enum ClientState {
    Handshake,
    Request,
    Connecting,
    Tunneling,
}
