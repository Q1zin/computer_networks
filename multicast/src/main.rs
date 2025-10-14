#[macro_use]
extern crate lazy_static;
extern crate socket2;

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle};
use std::time::Duration;
use std::mem::MaybeUninit;

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

pub const PORT: u16 = 7645;
lazy_static! {
    pub static ref IPV4: IpAddr = Ipv4Addr::new(224, 0, 0, 123).into();
    pub static ref IPV6: IpAddr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0x0123).into();
}

fn new_socket(addr: &SocketAddr) -> io::Result<Socket> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };

    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    
    socket.set_reuse_address(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(5000)))?;

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

fn multicast_listener(
    response: &'static str,
    client_done: Arc<AtomicBool>,
    addr: SocketAddr,
) -> JoinHandle<()> {
    let server_barrier = Arc::new(Barrier::new(2));
    let client_barrier = Arc::clone(&server_barrier);

    let join_handle = std::thread::Builder::new()
        .name(format!("{}:server", response))
        .spawn(move || {
            let listener = join_multicast(addr).expect("failed to create listener");
            println!("{}:server: joined: {}", response, addr);

            let responder = new_socket(&addr).expect("failed to create responder");
            let bind_addr = if addr.is_ipv4() {
                SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), 0)
            } else {
                SocketAddr::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0).into(), 0)
            };
            responder.bind(&SockAddr::from(bind_addr)).expect("failed to bind responder");
            
            server_barrier.wait();
            println!("{}:server: is ready", response);

            while !client_done.load(std::sync::atomic::Ordering::Relaxed) {
                let mut buf = [MaybeUninit::<u8>::uninit(); 64];

                match listener.recv_from(&mut buf) {
                    Ok((len, remote_addr_sock)) => {
                        let data = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len) };

                        let remote_socket_addr = remote_addr_sock
                            .as_socket()
                            .expect("failed to convert SockAddr to SocketAddr");

                        println!(
                            "{}:server: got data: {} from: {}",
                            response,
                            String::from_utf8_lossy(data),
                            remote_socket_addr
                        );

                        let multicast_addr = SockAddr::from(addr);
                        responder
                            .send_to(response.as_bytes(), &multicast_addr)
                            .expect("failed to respond");

                        println!("{}:server: sent response to multicast group: {}", response, addr);
                    }
                    Err(err) => {
                        println!("{}:server: got an error: {}", response, err);
                    }
                }
            }

            println!("{}:server: client is done", response);
        })
        .unwrap();

    client_barrier.wait();
    join_handle
}

fn new_sender(addr: &SocketAddr) -> io::Result<Socket> {
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

struct NotifyServer(Arc<AtomicBool>);
impl Drop for NotifyServer {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn test_multicast(test: &'static str, addr: IpAddr) {
    assert!(addr.is_multicast());
    let addr = SocketAddr::new(addr, PORT);

    let client_done = Arc::new(AtomicBool::new(false));
    let notify = NotifyServer(Arc::clone(&client_done));

    multicast_listener(test, client_done, addr);

    println!("{}:client: running", test);

    let message = b"Hello from client!";

    let socket = new_sender(&addr).expect("could not create sender!");
    
    let local_addr = socket.local_addr().expect("failed to get local addr");
    let local_socket_addr = local_addr.as_socket();
    println!("{}:client: bound to {:?}", test, local_socket_addr);
    
    let sock_addr = SockAddr::from(addr);
    socket.send_to(message, &sock_addr).expect("could not send_to!");

    std::thread::sleep(Duration::from_millis(100));

    let mut buf = [MaybeUninit::<u8>::uninit(); 64];

    match socket.recv_from(&mut buf) {
        Ok((len, _remote)) => {
            let data = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len) };
            let response = String::from_utf8_lossy(data);

            println!("{}:client: got data: {}", test, response);

            assert_eq!(test, response);
        }
        Err(err) => {
            println!("{}:client: had a problem: {}", test, err);
            assert!(false);
        }
    }

    drop(notify);
}

fn main() {
    println!("Hello, world!");
    test_multicast("ipv4", *IPV4);
    test_multicast("ipv6", *IPV6);
}