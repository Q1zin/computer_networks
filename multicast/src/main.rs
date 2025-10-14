use multicast::*;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use log::{LevelFilter, info};

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let instance_id = generate_instance_id();

    let server_running = Arc::new(AtomicBool::new(false));
    let client_running = Arc::new(AtomicBool::new(false));

    let server_flag = Arc::clone(&server_running);
    let server_id = instance_id.clone();
    let server_handle = thread::spawn(move || {
        server_thread(server_flag, server_id);
    });

    let client_flag = Arc::clone(&client_running);
    let client_id = instance_id.clone();
    let client_handle = thread::spawn(move || {
        client_thread(client_flag, client_id);
    });

    thread::sleep(Duration::from_secs(30));

    info!("\n=== Stopping ===\n");
    disconnect(Arc::clone(&client_running));
    stop_server(Arc::clone(&server_running));

    let _ = server_handle.join();
    let _ = client_handle.join();

    info!("Done!");
}
