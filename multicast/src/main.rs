use multicast::*;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use log::{LevelFilter, info};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "multicast")]
#[command(about = "Multicast UDP messenger", long_about = None)]
struct Args {
    #[arg(short, long, default_value = "239.255.255.250")]
    ip: String,

    #[arg(short, long, default_value_t = 8888)]
    port: u16,

    #[arg(short, long, default_value = "Hello from client")]
    message: String,

    #[arg(short, long, default_value_t = 30)]
    duration: u64,
}

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let args = Args::parse();

    let config = MulticastConfig::from_ip_string(&args.ip, args.port, args.message)
        .expect("Invalid IP address");

    let protocol = if config.is_ipv4() { "IPv4" } else { "IPv6" };
    info!("Configuration: IP={} ({}), Port={}, Message='{}'", config.ip, protocol, config.port, config.message);

    let instance_id = generate_instance_id();

    let server_running = Arc::new(AtomicBool::new(false));
    let client_running = Arc::new(AtomicBool::new(false));

    let server_flag = Arc::clone(&server_running);
    let server_id = instance_id.clone();
    let server_config = config.clone();
    let server_handle = thread::spawn(move || {
        server_thread(server_flag, server_id, server_config);
    });

    let client_flag = Arc::clone(&client_running);
    let client_id = instance_id.clone();
    let client_config = config.clone();
    let client_handle = thread::spawn(move || {
        client_thread(client_flag, client_id, client_config);
    });

    thread::sleep(Duration::from_secs(args.duration));

    info!("\n=== Stopping ===\n");
    disconnect(Arc::clone(&client_running));
    stop_server(Arc::clone(&server_running));

    let _ = server_handle.join();
    let _ = client_handle.join();

    info!("Done!");
}
