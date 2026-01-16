use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub name: String,
    pub player_count: usize,
}

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
pub fn get_available_games() -> Result<Vec<GameInfo>, String> {
    println!("Getting available games");

    Ok(vec![
        GameInfo {
            name: "Быстрая игра #1".to_string(),
            player_count: 3,
        },
        GameInfo {
            name: "Турнир профессионалов".to_string(),
            player_count: 8,
        },
        GameInfo {
            name: "Для новичков".to_string(),
            player_count: 2,
        },
        GameInfo {
            name: "Мега битва".to_string(),
            player_count: 12,
        },
        GameInfo {
            name: "Вечерняя игра".to_string(),
            player_count: 5,
        },
    ])
}

#[tauri::command]
pub fn join_game_as_player(game_name: String) -> Result<String, String> {
    println!("Joining game '{}' as player", game_name);

    Ok(format!("Joined '{}' as player", game_name))
}

#[tauri::command]
pub fn join_game_as_spectator(game_name: String) -> Result<String, String> {
    println!("Joining game '{}' as spectator", game_name);

    Ok(format!("Joined '{}' as spectator", game_name))
}

#[tauri::command]
pub fn send_steer(direction: Direction) -> Result<(), String> {
    println!("Steering to: {:?}", direction);

    Ok(())
}

#[tauri::command]
pub fn leave_game() -> Result<(), String> {
    println!("Leaving game");

    Ok(())
}

#[tauri::command]
pub fn become_spectator() -> Result<(), String> {
    println!("Becoming spectator");

    Ok(())
}

#[tauri::command]
pub fn get_game_state() -> Result<String, String> {
    println!("Getting game state");

    Err("Not implemented yet".to_string())
}

#[tauri::command]
pub fn exit_app(app: tauri::AppHandle) -> Result<(), String> {
    println!("Exiting application");
    app.exit(0);
    Ok(())
}
