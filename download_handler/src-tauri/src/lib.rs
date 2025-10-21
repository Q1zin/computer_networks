use client_api::{fetch_available_files, RemoteFileInfo};

#[tauri::command]
fn get_available_files(server_ip: &str, server_port: i32) -> Result<Vec<RemoteFileInfo>, String> {
    let server_addr = format!("{}:{}", server_ip, server_port);
    let available_files = fetch_available_files(&server_addr);
    match available_files {
        Ok(files) => Ok(files),
        Err(e) => Err(format!("Failed to fetch files: {}", e)),
    }
}



#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_available_files])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
