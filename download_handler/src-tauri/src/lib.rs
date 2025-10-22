use client_api::{upload_file, download_file, fetch_available_files, RemoteFileInfo};
use std::{env, path::Path};
use tauri::{AppHandle, Emitter};
use std::sync::OnceLock;

pub static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[tauri::command]
async fn get_available_files(
    server_ip: &str, 
    server_port: &str
) -> Result<Vec<RemoteFileInfo>, String> {
    let server_addr = format!("{}:{}", server_ip, server_port);
    let available_files = fetch_available_files(&server_addr);
    match available_files {
        Ok(files) => Ok(files),
        Err(e) => Err(format!("Failed to fetch files: {}", e)),
    }
}

#[tauri::command]
async fn download_file_front(
    server_ip: &str,
    server_port: &str,
    file_name: &str,
) -> Result<String, String> {
    let _ = env::home_dir()
        .map(|home| {
            let server_addr = format!("{}:{}", server_ip, server_port);
            let destination = home.join("Downloads").join(file_name);
            let result = download_file(file_name, &destination, &server_addr, |progress, instant, avg: f64| {
                let app_handle: &AppHandle = APP_HANDLE.get().expect("AppHandle not initialized");
                let file_name = destination
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                let response = ProgressData {
                    name: file_name,
                    progress,
                    instant,
                    avg,
                };

                app_handle.emit("download_progress", &response).unwrap();
                println!("Progress: {:6.2}% | Now: {:6.2} MB/s | Avg: {:6.2} MB/s | File: {}", progress, instant, avg, response.name);
            });

            
            match result {
                Ok(_) => Ok(format!(
                    "File '{}' downloaded successfully to {:?}",
                    file_name, destination
                )),
                Err(e) => Err(format!("Failed to download file '{}': {}", file_name, e)),
            }
        })
        .unwrap_or_else(|| Err("Home directory not found".to_string()));

    Ok("Download initiated".to_string())
}

#[derive(serde::Serialize)]
struct ProgressData {
    name: String,
    progress: f64,
    instant: f64,
    avg: f64,
}

#[tauri::command]
async fn upload_file_front(
    server_ip: &str,
    server_port: &str,
    file_path: &str,
) -> Result<String, String> {
    let server_addr = format!("{}:{}", server_ip, server_port);
    let source = Path::new(file_path);
    let result = upload_file(&source, &server_addr, |progress, instant, avg: f64| {
        let app_handle: &AppHandle = APP_HANDLE.get().expect("AppHandle not initialized");
        let file_name = source
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
        let response = ProgressData {
            name: file_name,
            progress,
            instant,
            avg,
        };

        app_handle.emit("upload_progress", &response).unwrap();
        println!("Progress: {:6.2}% | Now: {:6.2} MB/s | Avg: {:6.2} MB/s | File: {}", progress, instant, avg, response.name);
    });

    match result {
        Ok(_) => Ok(format!("File '{}' uploaded successfully from {:?}", file_path, source)),
        Err(e) => Err(format!("Failed to upload file '{}': {}", file_path, e)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle();
            APP_HANDLE.set(app_handle.clone()).unwrap();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_available_files,
            download_file_front,
            upload_file_front
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
