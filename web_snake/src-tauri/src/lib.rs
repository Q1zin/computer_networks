mod commands;

use commands::{
    create_new_game,
    get_available_games,
    join_game_as_player,
    join_game_as_spectator,
    exit_app,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            create_new_game,
            get_available_games,
            join_game_as_player,
            join_game_as_spectator,
            exit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
