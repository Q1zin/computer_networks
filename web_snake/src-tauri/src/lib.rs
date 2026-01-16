mod commands;
mod network;
mod snakes;

use tauri::Manager;
use network::NetworkService;

use commands::{
    search_games,
    create_new_game,
    join_game_as_player,
    join_game_as_spectator,
    exit_app,
    send_steer,
    leave_game,
    become_spectator,
    get_game_state,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(NetworkService::new().expect("Failed to init network service"))
        .setup(|app| {
            let network = app.state::<NetworkService>();
            network.start_polling(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search_games,
            create_new_game,
            join_game_as_player,
            join_game_as_spectator,
            exit_app,
            send_steer,
            leave_game,
            become_spectator,
            get_game_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
