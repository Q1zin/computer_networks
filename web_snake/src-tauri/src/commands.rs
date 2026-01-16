use crate::network::{GameStateDto, NetworkService};
use crate::snakes::NodeRole;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(i32)]
pub enum Direction {
    Up = 1,
    Down = 2,
    Left = 3,
    Right = 4,
}

#[tauri::command]
pub fn create_new_game(
    name: String,
    width: u32,
    height: u32,
    frequency: u32,
) -> Result<String, String> {
    println!(
        "Creating game: {} ({}x{}), frequency: {}ms",
        name, width, height, frequency
    );

    Ok(format!("Game '{}' created successfully", name))
}

#[tauri::command]
pub fn search_games(network: State<NetworkService>) -> Result<(), String> {
    network.send_discover().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn join_game_as_player(
    game_name: String,
    network: State<NetworkService>,
) -> Result<String, String> {
    println!("Joining game '{}' as player", game_name);
    network
        .join_game(&game_name, "Player", NodeRole::Normal)
        .map_err(|e| e.to_string())?;

    Ok(format!("Joined '{}' as player", game_name))
}

#[tauri::command]
pub fn join_game_as_spectator(
    game_name: String,
    network: State<NetworkService>,
) -> Result<String, String> {
    println!("Joining game '{}' as spectator", game_name);
    network
        .join_game(&game_name, "Viewer", NodeRole::Viewer)
        .map_err(|e| e.to_string())?;

    Ok(format!("Joined '{}' as spectator", game_name))
}

#[tauri::command]
pub fn send_steer(direction: Direction, network: State<NetworkService>) -> Result<(), String> {
    println!("Steering to: {:?}", direction);
    network
        .send_steer(direction as i32)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn leave_game(network: State<NetworkService>) -> Result<(), String> {
    println!("Leaving game");
    network.leave_game().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn become_spectator(network: State<NetworkService>) -> Result<(), String> {
    println!("Becoming spectator");
    network.become_spectator().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_game_state(network: State<NetworkService>) -> Result<Option<GameStateDto>, String> {
    println!("Getting game state");
    Ok(network.latest_state())
}

#[tauri::command]
pub fn exit_app(app: tauri::AppHandle) -> Result<(), String> {
    println!("Exiting application");
    app.exit(0);
    Ok(())
}
