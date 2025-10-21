use client_api::{upload_file, download_file, fetch_available_files, RemoteFileInfo};
use std::{env, path::Path};

#[tauri::command]
fn get_available_files(server_ip: &str, server_port: i32) -> Result<Vec<RemoteFileInfo>, String> {
    let server_addr = format!("{}:{}", server_ip, server_port);
    let available_files = fetch_available_files(&server_addr);
    match available_files {
        Ok(files) => Ok(files),
        Err(e) => Err(format!("Failed to fetch files: {}", e)),
    }
}

#[tauri::command]
fn download_file_front(
    server_ip: &str,
    server_port: i32,
    file_name: &str,
) -> Result<String, String> {
    let _ = env::home_dir()
        .map(|home| {
            let server_addr = format!("{}:{}", server_ip, server_port);
            let destination = home.join("Downloads").join(file_name);
            let result = download_file(file_name, &destination, &server_addr);
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

#[tauri::command]
fn upload_file_front(
    server_ip: &str,
    server_port: i32,
    file_path: &str,
) -> Result<String, String> {
    let server_addr = format!("{}:{}", server_ip, server_port);
    let source = Path::new(file_path);
    let result = upload_file(&source, &server_addr);
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
        .invoke_handler(tauri::generate_handler![
            get_available_files,
            download_file_front,
            upload_file_front
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
