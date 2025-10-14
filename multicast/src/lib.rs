use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::mem::MaybeUninit;
use uuid_rs::v4;
use lazy_static::lazy_static;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use log::{info, error};

lazy_static! {
    pub static ref MESSAGE_TEXT: Mutex<String> = Mutex::new(String::from("Hello from client"));
}

pub const MSG_TYPE_HEARTBEAT: u8 = 0;
pub const MSG_TYPE_DISCONNECT: u8 = 1;
pub const MAX_MESSAGE_SIZE: usize = 500;

#[derive(Clone, Debug)]
pub struct MulticastConfig {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub message: String,
}

impl Default for MulticastConfig {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::new(239, 255, 255, 250),
            port: 8888,
            message: String::from("Hello from client"),
        }
    }
}

pub fn generate_instance_id() -> String {
    v4!().to_string()
}

#[derive(Debug)]
pub struct Message {
    pub msg_type: u8,
    pub length: u16,
    pub uuid: String,
    pub text: String,
}

impl Message {
    pub fn serialize(&self) -> io::Result<Vec<u8>> {
        let mut buffer = Vec::new();
        
        buffer.push(self.msg_type);
        
        if self.text.len() > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Message too long: {} bytes (max {})", self.text.len(), MAX_MESSAGE_SIZE)
            ));
        }
        
        let text_bytes = self.text.as_bytes();
        let length = text_bytes.len() as u16;
        buffer.extend_from_slice(&length.to_be_bytes());
        buffer.extend_from_slice(self.uuid.as_bytes());
        buffer.extend_from_slice(text_bytes);
        
        Ok(buffer)
    }
    
    pub fn deserialize(data: &[u8]) -> io::Result<Self> {
        if data.len() < 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for message header"
            ));
        }
        
        let msg_type = data[0];
        
        let length = u16::from_be_bytes([data[1], data[2]]);
        
        if data.len() < 3 + 36 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for UUID"
            ));
        }
        
        let uuid = String::from_utf8_lossy(&data[3..39]).to_string();
        
        let text_end = 39 + length as usize;
        if data.len() < text_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Data too short for message text: expected {}, got {}", text_end, data.len())
            ));
        }
        
        let text = String::from_utf8_lossy(&data[39..text_end]).to_string();
        
        Ok(Message {
            msg_type,
            length,
            uuid,
            text,
        })
    }
}

pub fn new_socket(addr: &SocketAddr) -> io::Result<Socket> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };

    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(1000)))?;

    Ok(socket)
}

pub fn join_multicast(addr: SocketAddr) -> io::Result<Socket> {
    let ip_addr = addr.ip();
    let socket = new_socket(&addr)?;

    match ip_addr {
        IpAddr::V4(ref mdns_v4) => {
            socket.join_multicast_v4(mdns_v4, &Ipv4Addr::new(0, 0, 0, 0))?;
        }
        IpAddr::V6(ref mdns_v6) => {
            socket.join_multicast_v6(mdns_v6, 0)?;
            socket.set_only_v6(true)?;
        }
    };

    socket.bind(&SockAddr::from(addr))?;
    Ok(socket)
}

pub fn create_sender(addr: &SocketAddr) -> io::Result<Socket> {
    let socket = new_socket(addr)?;
    
    if addr.is_ipv4() {
        socket.set_multicast_if_v4(&Ipv4Addr::new(0, 0, 0, 0))?;
        socket.bind(&SockAddr::from(SocketAddr::new(
            Ipv4Addr::new(0, 0, 0, 0).into(),
            0,
        )))?;
    } else {
        socket.set_multicast_if_v6(0)?;
        socket.bind(&SockAddr::from(SocketAddr::new(
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0).into(),
            0,
        )))?;
    }
    
    Ok(socket)
}

pub fn server_thread(stop_flag: Arc<AtomicBool>, instance_id: String, config: MulticastConfig) {
    let mcast_addr = SocketAddr::new(config.ip.into(), config.port);
    
    info!("[SERVER] Starting multicast listener on {}:{}", config.ip, config.port);
    info!("[SERVER] Instance ID: {}", instance_id);
    
    let listener = match join_multicast(mcast_addr) {
        Ok(sock) => sock,
        Err(e) => {
            error!("[SERVER] Failed to join multicast group: {}", e);
            return;
        }
    };
    
    info!("[SERVER] Successfully joined multicast group, waiting for messages...");
    
    let mut buf = [MaybeUninit::<u8>::uninit(); 1024];
    
    while !stop_flag.load(Ordering::Relaxed) {
        match listener.recv_from(&mut buf) {
            Ok((len, remote_addr)) => {
                let data = unsafe {
                    std::slice::from_raw_parts(buf.as_ptr() as *const u8, len)
                };
                let remote_socket = remote_addr.as_socket();
                
                match Message::deserialize(data) {
                    Ok(msg) => {
                        if msg.uuid == instance_id {
                            continue;
                        }
                        
                        let msg_type_str = match msg.msg_type {
                            0 => "HEARTBEAT",
                            1 => "DISCONNECT",
                            _ => "UNKNOWN",
                        };
                        
                        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        info!("[SERVER] Received message from {:?}", remote_socket);
                        info!("Type: {} ({})", msg_type_str, msg.msg_type);
                        info!("Length: {} bytes", msg.length);
                        info!("UUID: {}", msg.uuid);
                        info!("Text: {}", msg.text);
                        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    }
                    Err(e) => {
                        error!("[SERVER] Failed to deserialize message: {}", e);
                    }
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) => {
                error!("[SERVER] Error receiving: {}", e);
            }
        }
    }

    info!("[SERVER] Shutting down");
}

pub fn stop_server(server_stop_flag: Arc<AtomicBool>) {
    info!("[STOP SERVER] Stopping server...");
    server_stop_flag.store(true, Ordering::Relaxed);
}

pub fn client_thread(stop_flag: Arc<AtomicBool>, instance_id: String, config: MulticastConfig) {
    let mcast_addr = SocketAddr::new(config.ip.into(), config.port);
    
    thread::sleep(Duration::from_millis(500));

    info!("[CLIENT] Starting multicast sender");

    let sender = match create_sender(&mcast_addr) {
        Ok(sock) => sock,
        Err(e) => {
            error!("[CLIENT] Failed to create sender socket: {}", e);
            return;
        }
    };
    
    let sock_addr = SockAddr::from(mcast_addr);
    let mut counter = 0;
    
    *MESSAGE_TEXT.lock().unwrap() = config.message.clone();
    
    info!("[CLIENT] Sending messages to {}:{} every 3 seconds...", config.ip, config.port);

    while !stop_flag.load(Ordering::Relaxed) {
        counter += 1;
        
        let msg_type = MSG_TYPE_HEARTBEAT;
        let text = MESSAGE_TEXT.lock().unwrap().clone();
        
        let message = Message {
            msg_type,
            length: text.len() as u16,
            uuid: instance_id.clone(),
            text: format!("{} #{}", text, counter),
        };
        
        match message.serialize() {
            Ok(data) => {
                match sender.send_to(&data, &sock_addr) {
                    Ok(bytes_sent) => {
                        let msg_type_str = match msg_type {
                            0 => "HEARTBEAT",
                            1 => "DISCONNECT",
                            _ => "UNKNOWN",
                        };
                        info!("[CLIENT] Sent {} bytes (type: {}): {}", bytes_sent, msg_type_str, message.text);
                    }
                    Err(e) => {
                        error!("[CLIENT] Failed to send: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("[CLIENT] Failed to serialize message: {}", e);
            }
        }
        
        for _ in 0..30 {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    
    send_disconnect_message(&sender, &sock_addr, &instance_id);

    info!("[CLIENT] Shutting down");
}

pub fn send_disconnect_message(sender: &Socket, sock_addr: &SockAddr, instance_id: &str) {
    let text = MESSAGE_TEXT.lock().unwrap().clone();
    
    let disconnect_msg = Message {
        msg_type: MSG_TYPE_DISCONNECT,
        length: text.len() as u16,
        uuid: instance_id.to_string(),
        text: format!("{} - Disconnecting", text),
    };
    
    match disconnect_msg.serialize() {
        Ok(data) => {
            match sender.send_to(&data, sock_addr) {
                Ok(bytes_sent) => {
                    info!("[CLIENT] Sent DISCONNECT message ({} bytes): {}", bytes_sent, disconnect_msg.text);
                }
                Err(e) => {
                    error!("[CLIENT] Failed to send disconnect: {}", e);
                }
            }
        }
        Err(e) => {
            error!("[CLIENT] Failed to serialize disconnect message: {}", e);
        }
    }
}

pub fn disconnect(client_stop_flag: Arc<AtomicBool>) {
    info!("[DISCONNECT] Stopping client and sending disconnect message...");
    client_stop_flag.store(true, Ordering::Relaxed);
}
