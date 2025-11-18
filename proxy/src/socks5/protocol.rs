use log::error;
use std::io::{Error, ErrorKind, Result};
use std::net::{SocketAddr, ToSocketAddrs};

pub const SOCKS_VERSION: u8 = 0x05;
pub const NO_AUTH: u8 = 0x00;
pub const CMD_CONNECT: u8 = 0x01;
pub const ATYP_IPV4: u8 = 0x01;
pub const ATYP_DOMAIN: u8 = 0x03;
pub const REP_SUCCESS: u8 = 0x00;
pub const REP_CONN_REFUSED: u8 = 0x05;
pub const BUFFER_SIZE: usize = 8192;

#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub resolved: SocketAddr,
    pub display: String,
}

pub fn parse_request(buf: &[u8]) -> Result<Option<RequestInfo>> {
    if buf.len() < 4 {
        return Ok(None);
    }

    let version = buf[0];
    let cmd = buf[1];
    let atyp = buf[3];

    if version != SOCKS_VERSION {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid version"));
    }

    if cmd != CMD_CONNECT {
        return Err(Error::new(ErrorKind::InvalidData, "Only CONNECT supported"));
    }

    match atyp {
        ATYP_IPV4 => parse_ipv4_request(buf),
        ATYP_DOMAIN => parse_domain_request(buf),
        _ => Err(Error::new(
            ErrorKind::InvalidData,
            "Unsupported address type",
        )),
    }
}

fn parse_ipv4_request(buf: &[u8]) -> Result<Option<RequestInfo>> {
    if buf.len() < 10 {
        return Ok(None);
    }
    let addr = format!(
        "{}.{}.{}.{}:{}",
        buf[4],
        buf[5],
        buf[6],
        buf[7],
        u16::from_be_bytes([buf[8], buf[9]])
    );
    let socket = match addr.parse() {
        Ok(s) => s,
        Err(_) => {
            error!("Invalid IPv4 address in request: {}", addr);
            return Err(Error::new(ErrorKind::InvalidData, "Invalid IPv4 address"));
        }
    };

    Ok(Some(RequestInfo {
        resolved: socket,
        display: addr,
    }))
}

fn parse_domain_request(buf: &[u8]) -> Result<Option<RequestInfo>> {
    if buf.len() < 5 {
        return Ok(None);
    }
    let len = buf[4] as usize;
    if buf.len() < 5 + len + 2 {
        return Ok(None);
    }
    let domain = String::from_utf8_lossy(&buf[5..5 + len]);
    let port = u16::from_be_bytes([buf[5 + len], buf[5 + len + 1]]);
    let addr = format!("{}:{}", domain, port);
    let mut addrs = addr.to_socket_addrs()?;
    if let Some(resolved) = addrs.next() {
        Ok(Some(RequestInfo {
            resolved,
            display: addr,
        }))
    } else {
        error!("No address resolved for domain: {}", domain);
        Err(Error::new(
            ErrorKind::AddrNotAvailable,
            "No address resolved",
        ))
    }
}

pub fn create_success_response() -> [u8; 10] {
    [SOCKS_VERSION, REP_SUCCESS, 0x00, 0x01, 0, 0, 0, 0, 0, 0]
}

pub fn create_refused_response() -> [u8; 10] {
    [
        SOCKS_VERSION,
        REP_CONN_REFUSED,
        0x00,
        0x01,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

pub fn create_auth_response() -> [u8; 2] {
    [SOCKS_VERSION, NO_AUTH]
}
