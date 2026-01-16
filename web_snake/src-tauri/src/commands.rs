use crate::game::GameManager;
use crate::network::{GameStateDto, NetworkService};
use crate::snakes::{GameConfig, GamePlayer, NodeRole, PlayerType};
use tauri::State;

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

    // Валидация параметров GameConfig по ТЗ:
    // - width: 10-100
    // - height: 10-100
    // - state_delay_ms: 100-3000
    if width < 10 || width > 100 {
        return Err(format!("Ширина поля должна быть от 10 до 100 (получено: {})", width));
    }
    if height < 10 || height > 100 {
        return Err(format!("Высота поля должна быть от 10 до 100 (получено: {})", height));
    }
    if frequency < 100 || frequency > 3000 {
        return Err(format!("Частота обновления должна быть от 100 до 3000 мс (получено: {})", frequency));
    }
    if name.trim().is_empty() {
        return Err("Имя игры не может быть пустым".to_string());
    }

    // Важное: при создании новой игры в том же процессе нужно сбросить хвосты прошлой
    // сессии (накопленные змейки, last_state в UI, прошлый my_id и т.п.).
    game_manager.reset();
    network.reset_local_session().map_err(|e| e.to_string())?;

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
        .create_game(config.clone(), host_player.clone())
        .map_err(|e| e.to_string())?;

    // Инициализируем NetworkService как Master
    network
        .init_as_master(name.clone(), config, host_player)
        .map_err(|e| e.to_string())?;

    // Настраиваем обработчики сетевых событий (Join, Steer)
    game_manager.setup_network_handlers(app.clone());

    // Запускаем игровой цикл с поддержкой broadcast через NetworkService
    game_manager.start_game_loop_with_network(app, network.inner().clone().into());

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
    direction: i32,
    network: State<NetworkService>,
    game_manager: State<GameManager>,
) -> Result<(), String> {
    let dir = match direction {
        1 => crate::snakes::Direction::Up,
        2 => crate::snakes::Direction::Down,
        3 => crate::snakes::Direction::Left,
        4 => crate::snakes::Direction::Right,
        _ => return Err(format!("Invalid direction: {}", direction)),
    };
    
    // Добавляем поворот в очередь для локальной игры
    if let Some(player_id) = game_manager.my_player_id() {
        game_manager.queue_steer(player_id, dir);
    }
    
    // Отправляем по сети
    network
        .send_steer(direction)
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
pub fn become_spectator(network: State<NetworkService>, game_manager: State<GameManager>) -> Result<(), String> {
    println!("Becoming spectator");
    // Если мы были MASTER, нужно остановить локальную симуляцию/loop немедленно.
    // Даже если network-layer уже перестанет слать пакеты, GameManager не должен продолжать тикать.
    game_manager.reset();
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
