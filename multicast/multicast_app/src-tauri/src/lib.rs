use multicast::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Emitter, State};
use serde::{Deserialize, Serialize};
use socket2::SockAddr;

#[derive(Clone, Serialize)]
struct MessageEvent {
    msg_type: String,
    uuid: String,
    text: String,
    timestamp: String,
}

struct AppState {
    server_running: Arc<AtomicBool>,
    client_running: Arc<AtomicBool>,
    instance_id: Mutex<Option<String>>,
    config: Mutex<Option<MulticastConfig>>,
}

#[derive(Deserialize)]
struct StartConfig {
    ip: String,
    port: u16,
    message: String,
    interface: Option<String>,
}

#[tauri::command]
fn start_multicast(
    config: StartConfig,
    state: State<AppState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    if state.server_running.load(Ordering::Relaxed) {
        return Err("Multicast already running".to_string());
    }

    let mcast_config = MulticastConfig::from_ip_string_with_interface(
        &config.ip,
        config.port,
        config.message.clone(),
        config.interface,
    )
    .map_err(|e| e.to_string())?;

    let instance_id = generate_instance_id();
    
    *state.instance_id.lock().unwrap() = Some(instance_id.clone());
    *state.config.lock().unwrap() = Some(mcast_config.clone());

    state.server_running.store(true, Ordering::Relaxed);
    state.client_running.store(true, Ordering::Relaxed);

    let server_flag = Arc::clone(&state.server_running);
    let server_id = instance_id.clone();
    let server_config = mcast_config.clone();
    let app_server = app.clone();
    
    thread::spawn(move || {
        let mcast_addr = std::net::SocketAddr::new(server_config.ip, server_config.port);
        
        let listener = match join_multicast(mcast_addr) {
            Ok(sock) => sock,
            Err(e) => {
                let _ = app_server.emit("multicast-error", format!("Failed to join: {}", e));
                return;
            }
        };

        let _ = app_server.emit("multicast-status", "Server started");
        
        let cleanup_flag = Arc::clone(&server_flag);
        thread::spawn(move || {
            while cleanup_flag.load(Ordering::Relaxed) {
                thread::sleep(std::time::Duration::from_secs(2));
                let removed = multicast::cleanup_inactive_devices(std::time::Duration::from_secs(14));
                if !removed.is_empty() {
                    println!("[CLEANUP] Removed {} inactive device(s)", removed.len());
                }
            }
        });
        
        let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1024];
        
        while server_flag.load(Ordering::Relaxed) {
            match listener.recv_from(&mut buf) {
                Ok((len, _)) => {
                    let data = unsafe {
                        std::slice::from_raw_parts(buf.as_ptr() as *const u8, len)
                    };
                    
                    if let Ok(msg) = Message::deserialize(data) {
                        if msg.uuid != server_id {
                            let msg_type_str = match msg.msg_type {
                                multicast::MSG_TYPE_HEARTBEAT => {
                                    multicast::update_device(msg.uuid.clone(), msg.text.clone());
                                    "HEARTBEAT"
                                },
                                multicast::MSG_TYPE_DISCONNECT => {
                                    multicast::remove_device(&msg.uuid);
                                    "DISCONNECT"
                                },
                                _ => "UNKNOWN",
                            };
                            
                            let event = MessageEvent {
                                msg_type: msg_type_str.to_string(),
                                uuid: msg.uuid,
                                text: msg.text,
                                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            };
                            
                            let _ = app_server.emit("multicast-message", event);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock 
                    || e.kind() == std::io::ErrorKind::TimedOut => {
                    continue;
                }
                Err(_) => continue,
            }
        }

        multicast::ACTIVE_DEVICES.lock().unwrap().clear();
        let _ = app_server.emit("multicast-status", "Server stopped");
    });

    let client_flag = Arc::clone(&state.client_running);
    let client_id = instance_id.clone();
    let client_config = mcast_config.clone();
    let app_client = app.clone();
    
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(500));
        
        let mcast_addr = std::net::SocketAddr::new(client_config.ip, client_config.port);
        let interface_ref = client_config.interface_name.as_deref();
        
        let sender = match create_sender(&mcast_addr, interface_ref) {
            Ok(sock) => sock,
            Err(e) => {
                let _ = app_client.emit("multicast-error", format!("Failed to create sender: {}", e));
                return;
            }
        };
        
        let _ = app_client.emit("multicast-status", "Client started");
        
        let sock_addr = SockAddr::from(mcast_addr);
        let mut counter = 0;
        
        *MESSAGE_TEXT.lock().unwrap() = client_config.message.clone();
        
        while client_flag.load(Ordering::Relaxed) {
            counter += 1;
            
            let text = MESSAGE_TEXT.lock().unwrap().clone();
            let message = Message {
                msg_type: MSG_TYPE_HEARTBEAT,
                length: text.len() as u16,
                uuid: client_id.clone(),
                text: format!("{} #{}", text, counter),
            };
            
            if let Ok(data) = message.serialize() {
                let _ = sender.send_to(&data, &sock_addr);
                let _ = app_client.emit("multicast-sent", counter);
            }
            
            for _ in 0..30 {
                if !client_flag.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        
        send_disconnect_message(&sender, &sock_addr, &client_id);
        let _ = app_client.emit("multicast-status", "Client stopped");
    });

    Ok(instance_id)
}

#[tauri::command]
fn stop_multicast(state: State<AppState>) -> Result<(), String> {
    if !state.server_running.load(Ordering::Relaxed) {
        return Err("Multicast not running".to_string());
    }

    state.client_running.store(false, Ordering::Relaxed);
    state.server_running.store(false, Ordering::Relaxed);

    *state.instance_id.lock().unwrap() = None;
    *state.config.lock().unwrap() = None;

    Ok(())
}

#[tauri::command]
fn update_message(message: String, state: State<AppState>) -> Result<(), String> {
    if let Some(ref mut config) = *state.config.lock().unwrap() {
        config.message = message.clone();
        *MESSAGE_TEXT.lock().unwrap() = message;
        Ok(())
    } else {
        Err("Multicast not running".to_string())
    }
}

#[tauri::command]
fn get_status(state: State<AppState>) -> bool {
    state.server_running.load(Ordering::Relaxed)
}

#[tauri::command]
fn get_instance_id(state: State<AppState>) -> Option<String> {
    state.instance_id.lock().unwrap().clone()
}

#[derive(Clone, Serialize)]
struct DeviceData {
    uuid: String,
    last_message: String,
    message_count: u32,
    seconds_since_seen: u64,
}

#[tauri::command]
fn get_active_devices() -> Vec<DeviceData> {
    let devices = multicast::get_active_devices();
    println!("[TAURI] get_active_devices called, found {} devices", devices.len());
    devices
        .iter()
        .map(|dev| {
            println!("[TAURI] Device: {} - {} msg, {} sec ago", dev.uuid, dev.message_count, dev.last_seen.elapsed().as_secs());
            DeviceData {
                uuid: dev.uuid.clone(),
                last_message: dev.last_message.clone(),
                message_count: dev.message_count,
                seconds_since_seen: dev.last_seen.elapsed().as_secs(),
            }
        })
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            server_running: Arc::new(AtomicBool::new(false)),
            client_running: Arc::new(AtomicBool::new(false)),
            instance_id: Mutex::new(None),
            config: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            start_multicast,
            stop_multicast,
            update_message,
            get_status,
            get_instance_id,
            get_active_devices
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
