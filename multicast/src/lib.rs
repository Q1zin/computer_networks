use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::mem::MaybeUninit;
use std::collections::HashMap;
use uuid_rs::v4;
use lazy_static::lazy_static;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use log::{info, error};
use if_addrs::get_if_addrs;

lazy_static! {
    pub static ref MESSAGE_TEXT: Mutex<String> = Mutex::new(String::from("Hello from client"));
    pub static ref ACTIVE_DEVICES: Mutex<HashMap<String, DeviceInfo>> = Mutex::new(HashMap::new());
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub uuid: String,
    pub last_seen: Instant,
    pub last_message: String,
    pub message_count: u32,
}

impl DeviceInfo {
    pub fn new(uuid: String, message: String) -> Self {
        Self {
            uuid,
            last_seen: Instant::now(),
            last_message: message,
            message_count: 1,
        }
    }

    pub fn update(&mut self, message: String) {
        self.last_seen = Instant::now();
        self.last_message = message;
        self.message_count += 1;
    }

    pub fn is_alive(&self, timeout: Duration) -> bool {
        self.last_seen.elapsed() < timeout
    }
}

pub const MSG_TYPE_HEARTBEAT: u8 = 0;
pub const MSG_TYPE_DISCONNECT: u8 = 1;
pub const MAX_MESSAGE_SIZE: usize = 500;

#[derive(Clone, Debug)]
pub struct MulticastConfig {
    pub ip: IpAddr,
    pub port: u16,
    pub message: String,
    pub interface_name: Option<String>,
}

impl Default for MulticastConfig {
    fn default() -> Self {
        Self {
            ip: IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)),
            port: 8888,
            message: String::from("Hello from client"),
            interface_name: None,
        }
    }
}

impl MulticastConfig {
    pub fn from_ip_string_with_interface(
        ip_str: &str, 
        port: u16, 
        message: String,
        interface_name: Option<String>
    ) -> io::Result<Self> {
        let ip: IpAddr = ip_str.parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid IP address: {}", e)))?;
        
        Ok(Self { 
            ip, 
            port, 
            message,
            interface_name,
        })
    }
    
    pub fn is_ipv4(&self) -> bool {
        self.ip.is_ipv4()
    }
    
    pub fn is_ipv6(&self) -> bool {
        self.ip.is_ipv6()
    }
}

pub fn generate_instance_id() -> String {
    v4!().to_string()
}

pub fn update_device(uuid: String, message: String) {
    let mut devices = ACTIVE_DEVICES.lock().unwrap();
    
    if let Some(device) = devices.get_mut(&uuid) {
        device.update(message);
        info!("[DEVICES] Updated device: {} (total: {})", uuid, devices.len());
    } else {
        info!("[DEVICES] New device connected: {} (total will be: {})", uuid, devices.len() + 1);
        devices.insert(uuid.clone(), DeviceInfo::new(uuid, message));
    }
}

pub fn remove_device(uuid: &str) {
    let mut devices = ACTIVE_DEVICES.lock().unwrap();
    if devices.remove(uuid).is_some() {
        info!("[DEVICES] Device disconnected: {}", uuid);
    }
}

pub fn cleanup_inactive_devices(timeout: Duration) -> Vec<String> {
    let mut devices = ACTIVE_DEVICES.lock().unwrap();
    let mut removed = Vec::new();
    
    devices.retain(|uuid, device| {
        if !device.is_alive(timeout) {
            info!("[DEVICES] Device timeout: {}", uuid);
            removed.push(uuid.clone());
            false
        } else {
            true
        }
    });
    
    removed
}

pub fn get_active_devices() -> Vec<DeviceInfo> {
    let devices = ACTIVE_DEVICES.lock().unwrap();
    let result: Vec<DeviceInfo> = devices.values().cloned().collect();
    info!("[LIB] get_active_devices returning {} devices", result.len());
    result
}

pub fn get_active_device_count() -> usize {
    let count = ACTIVE_DEVICES.lock().unwrap().len();
    info!("[LIB] get_active_device_count: {}", count);
    count
}

pub fn get_ipv6_interface_index(interface_name: Option<&str>) -> u32 {
    if let Some(name) = interface_name {
        match get_interface_index(name) {
            Ok(index) => {
                info!("[IPv6] Using specified interface: {} (index: {})", name, index);
                return index;
            }
            Err(e) => {
                error!("[IPv6] Failed to get index for interface '{}': {}. Falling back to auto-detection.", name, e);
            }
        }
    }
    
    find_ipv6_multicast_interface()
}

pub fn find_ipv6_multicast_interface() -> u32 {
    match get_if_addrs() {
        Ok(interfaces) => {
            for iface in interfaces.iter() {
                if let IpAddr::V6(ipv6_addr) = iface.addr.ip() {
                    if ipv6_addr.is_loopback() {
                        continue;
                    }
                    
                    if let Ok(index) = get_interface_index(&iface.name) {
                        return index;
                    }
                }
            }
            
            for iface in interfaces.iter() {
                if let IpAddr::V6(ipv6_addr) = iface.addr.ip() {
                    if !ipv6_addr.is_loopback() {
                        if let Ok(index) = get_interface_index(&iface.name) {
                            return index;
                        }
                    }
                }
            }
            
            error!("[IPv6] No suitable IPv6 interface found, using default (0)");
            0
        }
        Err(e) => {
            error!("[IPv6] Failed to get network interfaces: {}", e);
            0
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn get_interface_index(name: &str) -> io::Result<u32> {
    use std::ffi::CString;
    let c_name = CString::new(name)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    
    let index = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
    
    if index == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(index)
    }
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

pub fn join_multicast(addr: SocketAddr, interface_name: Option<&str>) -> io::Result<Socket> {
    let ip_addr = addr.ip();
    let socket = new_socket(&addr)?;

    match ip_addr {
        IpAddr::V4(ref mdns_v4) => {
            socket.join_multicast_v4(mdns_v4, &Ipv4Addr::new(0, 0, 0, 0))?;
        }
        IpAddr::V6(ref mdns_v6) => {
            let interface_index = get_ipv6_interface_index(interface_name);
            
            if interface_index != 0 {
                info!("[IPv6] Server using interface index {} for multicast join", interface_index);
            }
            
            socket.join_multicast_v6(mdns_v6, interface_index)?;
            socket.set_only_v6(true)?;
        }
    };

    socket.bind(&SockAddr::from(addr))?;
    Ok(socket)
}

pub fn create_sender(addr: &SocketAddr, interface_name: Option<&str>) -> io::Result<Socket> {
    let socket = new_socket(addr)?;
    
    if addr.is_ipv4() {
        socket.set_multicast_if_v4(&Ipv4Addr::UNSPECIFIED)?;
        socket.bind(&SockAddr::from(SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            0,
        )))?;
    } else {
        let is_link_local = if let IpAddr::V6(ref ipv6) = addr.ip() {
            ipv6.segments()[0] & 0xff0f == 0xff02
        } else {
            false
        };
        
        let interface_index = if is_link_local || interface_name.is_some() {
            get_ipv6_interface_index(interface_name)
        } else {
            0
        };
        
        if interface_index != 0 {
            info!("[IPv6] Using interface index {} for multicast", interface_index);
            socket.set_multicast_if_v6(interface_index)?;
        }
        
        socket.set_multicast_loop_v6(true)?;
        socket.bind(&SockAddr::from(SocketAddr::new(
            Ipv6Addr::UNSPECIFIED.into(),
            0,
        )))?;
    }
    
    Ok(socket)
}

pub fn server_thread(stop_flag: Arc<AtomicBool>, instance_id: String, config: MulticastConfig) {
    let mcast_addr = SocketAddr::new(config.ip, config.port);
    let protocol = if config.is_ipv4() { "IPv4" } else { "IPv6" };
    
    info!("[SERVER] Starting multicast listener on {}:{} ({})", config.ip, config.port, protocol);
    info!("[SERVER] Instance ID: {}", instance_id);
    
    let listener = match join_multicast(mcast_addr, config.interface_name.as_deref()) {
        Ok(sock) => sock,
        Err(e) => {
            error!("[SERVER] Failed to join multicast group: {}", e);
            return;
        }
    };
    
    info!("[SERVER] Successfully joined multicast group, waiting for messages...");
    
    let cleanup_flag = Arc::clone(&stop_flag);
    thread::spawn(move || {
        while !cleanup_flag.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(2));
            let removed = cleanup_inactive_devices(Duration::from_secs(14));
            if !removed.is_empty() {
                info!("[CLEANUP] Removed {} inactive device(s)", removed.len());
            }
        }
    });
    
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
                            MSG_TYPE_HEARTBEAT => {
                                update_device(msg.uuid.clone(), msg.text.clone());
                                "HEARTBEAT"
                            },
                            MSG_TYPE_DISCONNECT => {
                                remove_device(&msg.uuid);
                                "DISCONNECT"
                            },
                            _ => "UNKNOWN",
                        };

                        let device_count = get_active_device_count();
                        
                        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        info!("[SERVER] Received message from {:?}", remote_socket);
                        info!("Type: {} ({})", msg_type_str, msg.msg_type);
                        info!("Length: {} bytes", msg.length);
                        info!("UUID: {}", msg.uuid);
                        info!("Text: {}", msg.text);
                        info!("Active devices: {}", device_count);
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

    ACTIVE_DEVICES.lock().unwrap().clear();
    info!("[SERVER] Shutting down");
}

pub fn stop_server(server_stop_flag: Arc<AtomicBool>) {
    info!("[STOP SERVER] Stopping server...");
    server_stop_flag.store(true, Ordering::Relaxed);
}

pub fn client_thread(stop_flag: Arc<AtomicBool>, instance_id: String, config: MulticastConfig) {
    let mcast_addr = SocketAddr::new(config.ip, config.port);
    let protocol = if config.is_ipv4() { "IPv4" } else { "IPv6" };
    
    thread::sleep(Duration::from_millis(500));

    info!("[CLIENT] Starting multicast sender ({})", protocol);
    
    let interface_ref = config.interface_name.as_deref();

    let sender = match create_sender(&mcast_addr, interface_ref) {
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
