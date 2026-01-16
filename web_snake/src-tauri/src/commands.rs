use crate::game::GameManager;
use crate::network::{GameStateDto, NetworkService};
use crate::snakes::{GameConfig, GamePlayer, NodeRole, PlayerType};
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
    app: tauri::AppHandle,
    game_manager: State<GameManager>,
    network: State<NetworkService>,
) -> Result<String, String> {
    println!(
        "Creating game: {} ({}x{}), frequency: {}ms",
        name, width, height, frequency
    );

    let config = GameConfig {
        width: Some(width as i32),
        height: Some(height as i32),
        food_static: Some(1),
        state_delay_ms: Some(frequency as i32),
    };

    let host_player = GamePlayer {
        name: "Host".to_string(),
        id: 1,
        ip_address: None,
        port: None,
        role: NodeRole::Master as i32,
        r#type: Some(PlayerType::Human as i32),
        score: 0,
    };

    // Создаем игру в GameManager
    game_manager
        .create_game(config, host_player.clone())
        .map_err(|e| e.to_string())?;

    // Инициализируем NetworkService как Master
    network
        .init_as_master(host_player)
        .map_err(|e| e.to_string())?;

    // Запускаем игровой цикл
    game_manager.start_game_loop(app);

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
pub fn send_steer(
    direction: Direction,
    network: State<NetworkService>,
    game_manager: State<GameManager>,
) -> Result<(), String> {
    println!("Steering to: {:?}", direction);
    
    // Добавляем поворот в очередь для локальной игры
    if let Some(player_id) = game_manager.my_player_id() {
        let dir = match direction {
            Direction::Up => crate::snakes::Direction::Up,
            Direction::Down => crate::snakes::Direction::Down,
            Direction::Left => crate::snakes::Direction::Left,
            Direction::Right => crate::snakes::Direction::Right,
        };
        game_manager.queue_steer(player_id, dir);
    }
    
    // Отправляем по сети
    network
        .send_steer(direction as i32)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn leave_game(
    network: State<NetworkService>,
    game_manager: State<GameManager>,
) -> Result<(), String> {
    println!("Leaving game");
    game_manager.reset();
    network.leave_game().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn become_spectator(network: State<NetworkService>) -> Result<(), String> {
    println!("Becoming spectator");
    network.become_spectator().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_game_state(
    network: State<NetworkService>,
    game_manager: State<GameManager>,
) -> Result<Option<GameStateDto>, String> {
    println!("Getting game state");
    
    // Сначала пытаемся получить состояние из локальной игры
    if let Some(_state) = game_manager.get_state() {
        // Преобразуем в DTO (используем существующую функцию из network)
        return Ok(network.latest_state());
    }
    
    // Если локальной игры нет, получаем из сети
    Ok(network.latest_state())
}

#[tauri::command]
pub fn exit_app(app: tauri::AppHandle) -> Result<(), String> {
    println!("Exiting application");
    app.exit(0);
    Ok(())
}
