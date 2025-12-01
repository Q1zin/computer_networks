#[derive(Debug)]
pub enum ClientState {
    Handshake,
    Request,
    Resolving,
    Connecting,
    Tunneling,
}
