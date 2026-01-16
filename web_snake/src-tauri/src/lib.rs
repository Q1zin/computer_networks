mod commands;
mod game;
mod network;
mod snakes;

use tauri::Manager;
use tauri::Listener;
use network::NetworkService;
use game::GameManager;

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
        .manage(GameManager::new())
        .setup(|app| {
            let network = app.state::<NetworkService>();
            network.start_polling(app.handle());

            // Если deputy стал MASTER (таймаут/отвал прошлого мастера) — поднимаем симуляцию
            // из последнего полученного снапшота и начинаем рассылку State.
            let app_handle = app.handle().clone();
            let app_for_listener = app_handle.clone();
            app_handle.listen("became-master", move |event| {
                #[derive(serde::Deserialize)]
                struct BecameMasterEvent {
                    old_master_id: i32,
                }

                let old_master_id = serde_json::from_str::<BecameMasterEvent>(event.payload())
                    .map(|p| p.old_master_id)
                    .unwrap_or(0);

                println!("[became-master] Event received, old_master_id={}", old_master_id);

                // Важно: callback слушателя может выполняться синхронно внутри обработки сети,
                // где уже удерживаются mutex-ы state_manager/players. Чтобы не словить deadlock,
                // делаем реальный takeover асинхронно.
                let app_async = app_for_listener.clone();
                std::thread::spawn(move || {
                    println!("[became-master] Takeover thread started");
                    let network = app_async.state::<NetworkService>();
                    let game_manager = app_async.state::<GameManager>();

                    let snapshot = match network.latest_state_raw() {
                        Some(s) => {
                            println!("[became-master] Got snapshot with state_order={}", s.state_order);
                            s
                        }
                        None => {
                            eprintln!("became-master: no last_state snapshot; cannot resume game");
                            return;
                        }
                    };

                    let config = network.current_game_config_raw().unwrap_or(crate::snakes::GameConfig {
                        width: Some(40),
                        height: Some(30),
                        food_static: Some(1),
                        state_delay_ms: Some(1000),
                    });

                    let my_id = network.my_player_id();
                    println!("[became-master] my_id={}, config delay={}ms", my_id, config.state_delay_ms.unwrap_or(0));

                    // На всякий случай останавливаем любые старые циклы (если были)
                    game_manager.reset();

                    if let Err(e) = game_manager.takeover_from_snapshot(config, snapshot, my_id) {
                        eprintln!("Failed to takeover game as master: {e}");
                        return;
                    }
                    println!("[became-master] takeover_from_snapshot succeeded");

                    if old_master_id != 0 {
                        game_manager.change_snake_to_zombie(old_master_id);
                    }

                    // Нужны обработчики join/steer/zombie для нового мастера
                    game_manager.setup_network_handlers(app_async.clone());

                    // Стартуем основной цикл с сетевой рассылкой
                    println!("[became-master] Starting game loop with network");
                    game_manager.start_game_loop_with_network(
                        app_async.clone(),
                        network.inner().clone().into(),
                    );
                    println!("[became-master] start_game_loop_with_network returned");
                });
            });
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
