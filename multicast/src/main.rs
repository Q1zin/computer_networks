use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::mem::MaybeUninit;
use uuid_rs::v4;

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

const MCAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 123);
const PORT: u16 = 7645;

fn new_socket(addr: &SocketAddr) -> io::Result<Socket> {
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

fn join_multicast(addr: SocketAddr) -> io::Result<Socket> {
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

fn create_sender(addr: &SocketAddr) -> io::Result<Socket> {
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

fn server_thread(stop_flag: Arc<AtomicBool>, instance_id: String) {
    let mcast_addr = SocketAddr::new(MCAST_ADDR.into(), PORT);
    
    println!("[SERVER] Starting multicast listener on {}:{}", MCAST_ADDR, PORT);
    println!("[SERVER] Instance ID: {}", instance_id);
    
    let listener = match join_multicast(mcast_addr) {
        Ok(sock) => sock,
        Err(e) => {
            eprintln!("[SERVER] Failed to join multicast group: {}", e);
            return;
        }
    };
    
    println!("[SERVER] Successfully joined multicast group, waiting for messages...");
    
    let mut buf = [MaybeUninit::<u8>::uninit(); 1024];
    
    while !stop_flag.load(Ordering::Relaxed) {
        match listener.recv_from(&mut buf) {
            Ok((len, remote_addr)) => {
                let data = unsafe {
                    std::slice::from_raw_parts(buf.as_ptr() as *const u8, len)
                };
                let message = String::from_utf8_lossy(data);
                let remote_socket = remote_addr.as_socket();
                
                if message.starts_with(&instance_id) {
                    continue;
                }
                
                println!("[SERVER] Received from {:?}: {}", remote_socket, message);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) => {
                eprintln!("[SERVER] Error receiving: {}", e);
            }
        }
    }
    
    println!("[SERVER] Shutting down");
}

fn client_thread(stop_flag: Arc<AtomicBool>, instance_id: String) {
    let mcast_addr = SocketAddr::new(MCAST_ADDR.into(), PORT);
    
    thread::sleep(Duration::from_millis(500));
    
    println!("[CLIENT] Starting multicast sender");
    
    let sender = match create_sender(&mcast_addr) {
        Ok(sock) => sock,
        Err(e) => {
            eprintln!("[CLIENT] Failed to create sender socket: {}", e);
            return;
        }
    };
    
    let sock_addr = SockAddr::from(mcast_addr);
    let mut counter = 0;
    
    println!("[CLIENT] Sending messages to {}:{} every 3 seconds...", MCAST_ADDR, PORT);
    
    while !stop_flag.load(Ordering::Relaxed) {
        counter += 1;
        let message = format!("{}Message #{} from client", instance_id, counter);

        match sender.send_to(message.as_bytes(), &sock_addr) {
            Ok(bytes_sent) => {
                println!("[CLIENT] Sent {} bytes: {}", bytes_sent, message);
            }
            Err(e) => {
                eprintln!("[CLIENT] Failed to send: {}", e);
            }
        }
        
        for _ in 0..30 {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    
    println!("[CLIENT] Shutting down");
}

fn generate_instance_id() -> String {
    v4!().to_string()
}

fn main() {
    let instance_id = generate_instance_id();

    let running = Arc::new(AtomicBool::new(false));

    let server_flag = Arc::clone(&running);
    let server_id = instance_id.clone();
    let server_handle = thread::spawn(move || {
        server_thread(server_flag, server_id);
    });

    let client_flag = Arc::clone(&running);
    let client_id = instance_id.clone();
    let client_handle = thread::spawn(move || {
        client_thread(client_flag, client_id);
    });

    thread::sleep(Duration::from_secs(30));

    println!("\n=== Stopping ===");
    running.store(true, Ordering::Relaxed);

    let _ = server_handle.join();
    let _ = client_handle.join();

    println!("Done!");
}
