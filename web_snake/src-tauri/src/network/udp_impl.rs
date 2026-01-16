use super::protocol::NetworkProtocol;
use crate::snakes::{GameMessage, NodeRole};
use anyhow::{anyhow, Result};
use prost::Message;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Mutex;

pub struct UdpNetwork {
    main_socket: UdpSocket,
    multicast_rx: UdpSocket,
    multicast_addr: SocketAddr,
    role: Mutex<NodeRole>,
    recv_buf: Mutex<Vec<u8>>,
}

impl UdpNetwork {
    pub fn new() -> Result<Self> {
        let main_socket = UdpSocket::bind("0.0.0.0:0")?;
        main_socket.set_nonblocking(true)?;

        let mcast = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        mcast.set_reuse_address(true)?;
        mcast.set_reuse_port(true)?;
        mcast.bind(&SocketAddr::from(([0, 0, 0, 0], 9192)).into())?;
        let multicast_rx: UdpSocket = mcast.into();
        multicast_rx.set_nonblocking(true)?;
        multicast_rx.join_multicast_v4(&Ipv4Addr::new(239, 192, 0, 4), &Ipv4Addr::UNSPECIFIED)?;

        Ok(Self {
            main_socket,
            multicast_rx,
            multicast_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 192, 0, 4)), 9192),
            role: Mutex::new(NodeRole::Normal),
            recv_buf: Mutex::new(vec![0u8; 65_536]),
        })
    }
}

impl NetworkProtocol for UdpNetwork {
    fn serialize(&self, msg: &GameMessage) -> Vec<u8> {
        let mut buf = Vec::new();
        msg.encode(&mut buf).expect("Encoding failed (serialize)");
        buf
    }

    fn deserialize(&self, bytes: &[u8]) -> Result<GameMessage> {
        GameMessage::decode(bytes).map_err(|e| anyhow!("Decode error: {}", e))
    }

    fn send_unicast(&self, addr: SocketAddr, msg: GameMessage) -> Result<()> {
        let buf = self.serialize(&msg);
        self.main_socket.send_to(&buf, addr)?;
        Ok(())
    }

    fn send_multicast(&self, msg: GameMessage) -> Result<()> {
        if *self.role.lock().expect("role mutex poisoned") == NodeRole::Master {
            let buf = self.serialize(&msg);
            self.main_socket.send_to(&buf, self.multicast_addr)?;
        }
        Ok(())
    }

    fn poll_receive(&self) -> Result<Option<(GameMessage, SocketAddr)>> {
        let mut buf = self.recv_buf.lock().expect("recv_buf mutex poisoned");

        match self.main_socket.recv_from(&mut buf) {
            Ok((len, addr)) => {
                let msg = self.deserialize(&buf[..len])?;
                return Ok(Some((msg, addr)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }

        match self.multicast_rx.recv_from(&mut buf) {
            Ok((len, addr)) => {
                let msg = self.deserialize(&buf[..len])?;
                Ok(Some((msg, addr)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn get_local_addr(&self) -> Result<SocketAddr> {
        Ok(self.main_socket.local_addr()?)
    }

    fn set_role(&self, role: NodeRole) {
        *self.role.lock().expect("role mutex poisoned") = role;
    }
}
