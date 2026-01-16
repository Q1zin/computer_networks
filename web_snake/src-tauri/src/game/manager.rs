use crate::game::field::{FieldImpl, GameField};
use crate::network::game_state_to_dto;
use crate::snakes::{Direction, GameConfig, GamePlayers, GameState, GamePlayer};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener, Emitter};

pub struct GameManager {
    field: Arc<Mutex<Option<FieldImpl>>>,
    pending_steers: Arc<Mutex<HashMap<i32, Direction>>>,
    my_player_id: Arc<Mutex<Option<i32>>>,
    last_tick: Arc<Mutex<Instant>>,
    running: Arc<AtomicBool>,
    config: Arc<Mutex<Option<GameConfig>>>,
    handlers_registered: AtomicBool,
}

impl GameManager {
    pub fn new() -> Self {
        Self {
            field: Arc::new(Mutex::new(None)),
            pending_steers: Arc::new(Mutex::new(HashMap::new())),
            my_player_id: Arc::new(Mutex::new(None)),
            last_tick: Arc::new(Mutex::new(Instant::now())),
            running: Arc::new(AtomicBool::new(false)),
            config: Arc::new(Mutex::new(None)),
            handlers_registered: AtomicBool::new(false),
        }
    }

    /// Создание новой игры (для Master)
    pub fn create_game(&self, config: GameConfig, host_player: GamePlayer) -> Result<()> {
        let player_id = host_player.id;
        let players = GamePlayers {
            players: vec![host_player],
        };
        
        let field = FieldImpl::new(config.clone(), players);
        *self.field.lock().unwrap() = Some(field);
        *self.config.lock().unwrap() = Some(config);
        *self.my_player_id.lock().unwrap() = Some(player_id);
        Ok(())
    }

    /// Восстановление игры из снапшота и запуск в режиме MASTER (для deputy promotion).
    pub fn takeover_from_snapshot(
        &self,
        config: GameConfig,
        snapshot: GameState,
        my_player_id: i32,
    ) -> Result<()> {
        let field = FieldImpl::from_snapshot(config.clone(), snapshot)?;
        *self.field.lock().unwrap() = Some(field);
        *self.config.lock().unwrap() = Some(config);
        *self.my_player_id.lock().unwrap() = Some(my_player_id);
        *self.last_tick.lock().unwrap() = Instant::now();
        Ok(())
    }

    /// Добавление нового игрока в игру (для Master)
    pub fn add_player(&self, player_name: String) -> Result<i32> {
        let mut field_guard = self.field.lock().unwrap();
        let field = field_guard.as_mut().ok_or_else(|| anyhow!("Game not initialized"))?;
        
        field.place_new_snake(player_name)
    }

    /// Обработка поворота змейки
    pub fn queue_steer(&self, player_id: i32, direction: Direction) {
        let mut steers = self.pending_steers.lock().unwrap();
        steers.insert(player_id, direction);
    }

    /// Обновление состояния игры (только для Master)
    pub fn tick(&self, delay_ms: u32) -> Result<Option<GameState>> {
        let mut last_tick = self.last_tick.lock().unwrap();
        let elapsed = last_tick.elapsed();
        
        if elapsed < Duration::from_millis(delay_ms as u64) {
            return Ok(None);
        }
        
        *last_tick = Instant::now();
        
        let mut field_guard = self.field.lock().unwrap();
        let field = field_guard.as_mut().ok_or_else(|| anyhow!("Game not initialized"))?;
        
        let mut steers_guard = self.pending_steers.lock().unwrap();
        let steers = std::mem::take(&mut *steers_guard);
        
        let new_state = field.update(steers)?;
        Ok(Some(new_state))
    }

    /// Получение текущего состояния
    pub fn get_state(&self) -> Option<GameState> {
        let field_guard = self.field.lock().unwrap();
        field_guard.as_ref().map(|f| f.get_current_state())
    }

    /// Установка ID текущего игрока
    pub fn set_my_player_id(&self, id: i32) {
        *self.my_player_id.lock().unwrap() = Some(id);
    }

    /// Получение ID текущего игрока
    pub fn my_player_id(&self) -> Option<i32> {
        *self.my_player_id.lock().unwrap()
    }

    /// Проверка, что игра полная
    pub fn is_full(&self) -> bool {
        let field_guard = self.field.lock().unwrap();
        field_guard.as_ref().map(|f| f.is_full()).unwrap_or(false)
    }

    /// Изменение состояния змейки на ZOMBIE (змейка продолжает двигаться сама)
    pub fn change_snake_to_zombie(&self, player_id: i32) {
        let mut field_guard = self.field.lock().unwrap();
        if let Some(field) = field_guard.as_mut() {
            field.change_snake_to_zombie(player_id);
        }
    }

    /// Обработка смерти змейки
    pub fn handle_death(&self, snake_id: i32) {
        let mut field_guard = self.field.lock().unwrap();
        if let Some(field) = field_guard.as_mut() {
            field.handle_death(snake_id);
        }
    }

    /// Сброс игры
    pub fn reset(&self) {
        self.running.store(false, Ordering::SeqCst);
        *self.field.lock().unwrap() = None;
        self.pending_steers.lock().unwrap().clear();
        *self.my_player_id.lock().unwrap() = None;
        *self.last_tick.lock().unwrap() = Instant::now();
        *self.config.lock().unwrap() = None;
    }

    /// Запуск игрового цикла (только для Master)
    pub fn start_game_loop(&self, app: AppHandle) {
        if self.running.swap(true, Ordering::SeqCst) {
            println!("Game loop already running");
            return;
        }

        let field = Arc::clone(&self.field);
        let pending_steers = Arc::clone(&self.pending_steers);
        let last_tick = Arc::clone(&self.last_tick);
        let running = Arc::clone(&self.running);
        let config = Arc::clone(&self.config);
        let app_clone = app.clone();

        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                let delay_ms = {
                    let cfg_guard = config.lock().unwrap();
                    cfg_guard
                        .as_ref()
                        .and_then(|c| c.state_delay_ms)
                        .unwrap_or(500)
                };

                let should_update = {
                    let last_tick_guard = last_tick.lock().unwrap();
                    last_tick_guard.elapsed() >= Duration::from_millis(delay_ms as u64)
                };

                if should_update {
                    let mut field_guard = field.lock().unwrap();
                    if let Some(fld) = field_guard.as_mut() {
                        let mut steers_guard = pending_steers.lock().unwrap();
                        let steers = std::mem::take(&mut *steers_guard);
                        drop(steers_guard);

                        match fld.update(steers) {
                            Ok(new_state) => {
                                // Получаем config для передачи в DTO
                                let cfg_guard = config.lock().unwrap();
                                let game_config = cfg_guard.as_ref().cloned().unwrap_or(GameConfig {
                                    width: Some(40),
                                    height: Some(30),
                                    food_static: Some(1),
                                    state_delay_ms: Some(1000),
                                });
                                drop(cfg_guard);
                                
                                // Отправляем State через event используя DTO
                                let state_dto = game_state_to_dto(&new_state, &game_config);
                                let _ = app_clone.emit("game-state", state_dto);
                            }
                            Err(e) => {
                                eprintln!("Game update error: {}", e);
                            }
                        }

                        *last_tick.lock().unwrap() = Instant::now();
                    }
                }

                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }

    /// Запуск игрового цикла с поддержкой отправки State по сети (только для Master)
    pub fn start_game_loop_with_network(&self, app: AppHandle, network: Arc<crate::network::NetworkService>) {
        if self.running.swap(true, Ordering::SeqCst) {
            println!("Game loop already running");
            return;
        }

        let field = Arc::clone(&self.field);
        let pending_steers = Arc::clone(&self.pending_steers);
        let last_tick = Arc::clone(&self.last_tick);
        let running = Arc::clone(&self.running);
        let config = Arc::clone(&self.config);
        let my_player_id = Arc::clone(&self.my_player_id);
        let app_clone = app.clone();

        std::thread::spawn(move || {
            println!("[game-loop-net] Thread started");
            let mut tick_count = 0u32;
            let mut master_snake_dead = false;
            
            while running.load(Ordering::SeqCst) {
                let delay_ms = {
                    let cfg_guard = config.lock().unwrap();
                    cfg_guard
                        .as_ref()
                        .and_then(|c| c.state_delay_ms)
                        .unwrap_or(500)
                };

                let should_update = {
                    let last_tick_guard = last_tick.lock().unwrap();
                    last_tick_guard.elapsed() >= Duration::from_millis(delay_ms as u64)
                };

                if should_update {
                    tick_count += 1;
                    let mut field_guard = field.lock().unwrap();
                    if let Some(fld) = field_guard.as_mut() {
                        let mut steers_guard = pending_steers.lock().unwrap();
                        let steers = std::mem::take(&mut *steers_guard);
                        drop(steers_guard);

                        // Обновляем флаг is_full для корректной обработки JoinMsg и AnnouncementMsg
                        network.set_is_full(fld.is_full());

                        match fld.update(steers) {
                            Ok(new_state) => {
                                if tick_count <= 3 {
                                    println!("[game-loop-net] Tick #{}, state_order={}", tick_count, new_state.state_order);
                                }
                                
                                // Получаем актуальные роли игроков
                                let players_snapshot = network.get_players();
                                let my_id = my_player_id.lock().unwrap().unwrap_or(0);
                                
                                // Проверяем, есть ли ещё активные игроки (не Viewer) с живыми змейками
                                let active_player_ids: Vec<i32> = players_snapshot.players.iter()
                                    .filter(|p| p.role != crate::snakes::NodeRole::Viewer as i32)
                                    .map(|p| p.id)
                                    .collect();
                                
                                let alive_active_snakes: Vec<i32> = new_state.snakes.iter()
                                    .filter(|s| active_player_ids.contains(&s.player_id))
                                    .map(|s| s.player_id)
                                    .collect();
                                
                                // Если не осталось активных игроков со змейками - игра окончена
                                if alive_active_snakes.is_empty() {
                                    println!("[game-loop-net] No active players with snakes left - game over!");
                                    running.store(false, Ordering::SeqCst);
                                    
                                    // Уведомляем всех о завершении игры
                                    let _ = app_clone.emit("game-over", "All players died");
                                    
                                    // Сбрасываем сессию в Lobby чтобы остановить announcements
                                    let _ = network.reset_local_session();
                                    
                                    break;
                                }
                                
                                // Проверяем, умерла ли змейка master'а
                                let my_snake_alive = new_state.snakes.iter().any(|s| s.player_id == my_id);
                                
                                if !my_snake_alive && !master_snake_dead && my_id != 0 {
                                    // Змейка master'а умерла - нужно стать viewer и передать роль deputy
                                    println!("[game-loop-net] Master's snake died! Becoming spectator...");
                                    master_snake_dead = true;
                                    
                                    // Останавливаем game loop - мы больше не master
                                    running.store(false, Ordering::SeqCst);
                                    
                                    // Вызываем become_spectator для handoff на deputy
                                    let network_clone = network.clone();
                                    std::thread::spawn(move || {
                                        if let Err(e) = network_clone.become_spectator() {
                                            eprintln!("Failed to become spectator after death: {}", e);
                                        }
                                    });
                                    
                                    // Выходим из цикла
                                    break;
                                }
                                
                                // Получаем config для передачи в DTO
                                let cfg_guard = config.lock().unwrap();
                                let game_config = cfg_guard.as_ref().cloned().unwrap_or(GameConfig {
                                    width: Some(40),
                                    height: Some(30),
                                    food_static: Some(1),
                                    state_delay_ms: Some(1000),
                                });
                                drop(cfg_guard);
                                
                                // Роли (в т.ч. DEPUTY) назначаются сетевым слоем.
                                // Перед отрисовкой накладываем роли на State, чтобы не было
                                // ситуации "2+ игроков, но в State нет deputy".
                                let mut state_for_ui = new_state.clone();
                                let mut role_by_id = std::collections::HashMap::<i32, i32>::new();
                                for p in &players_snapshot.players {
                                    role_by_id.insert(p.id, p.role);
                                }
                                for p in &mut state_for_ui.players.players {
                                    if let Some(role) = role_by_id.get(&p.id) {
                                        p.role = *role;
                                    }
                                }

                                // Отправляем State через event используя DTO (для локального фронтенда)
                                let state_dto = game_state_to_dto(&state_for_ui, &game_config);
                                let _ = app_clone.emit("game-state", state_dto);
                                
                                // Отправляем State по сети всем игрокам
                                if let Err(e) = network.broadcast_state(&new_state) {
                                    eprintln!("Failed to broadcast state: {}", e);
                                }
                            }
                            Err(e) => {
                                eprintln!("Game update error: {}", e);
                            }
                        }

                        *last_tick.lock().unwrap() = Instant::now();
                    } else {
                        println!("[game-loop-net] field is None!");
                    }
                }

                std::thread::sleep(Duration::from_millis(10));
            }
            println!("[game-loop-net] Thread stopped");
        });
    }

    /// Настройка обработчиков сетевых событий для Master
    pub fn setup_network_handlers(&self, app: AppHandle) {
        if self.handlers_registered.swap(true, Ordering::SeqCst) {
            // Эти обработчики должны регистрироваться один раз на всё приложение.
            // Иначе при create_new_game() будет накапливаться несколько слушателей,
            // и на один join/steer появятся дубликаты змей.
            return;
        }

        let field_for_join = Arc::clone(&self.field);
        let field_for_zombie = Arc::clone(&self.field);
        let field_for_left = Arc::clone(&self.field);
        let pending_steers_for_steer = Arc::clone(&self.pending_steers);

        // Подписываемся на событие player-joined для добавления игроков
        app.listen("player-joined", move |event| {
            #[derive(serde::Deserialize)]
            struct JoinEvent {
                player_name: String,
                player_id: i32,
            }
            
            if let Ok(join_event) = serde_json::from_str::<JoinEvent>(event.payload()) {
                let mut field_guard = field_for_join.lock().unwrap();
                if let Some(field) = field_guard.as_mut() {
                    let _ = field.place_new_snake(join_event.player_name);
                }
            }
        });

        // Подписываемся на событие player-became-zombie для изменения состояния змейки
        app.listen("player-became-zombie", move |event| {
            #[derive(serde::Deserialize)]
            struct ZombieEvent {
                player_id: i32,
            }
            
            if let Ok(zombie_event) = serde_json::from_str::<ZombieEvent>(event.payload()) {
                let mut field_guard = field_for_zombie.lock().unwrap();
                if let Some(field) = field_guard.as_mut() {
                    field.change_snake_to_zombie(zombie_event.player_id);
                }
            }
        });

        // Подписываемся на событие player-left для удаления игрока из field
        app.listen("player-left", move |event| {
            #[derive(serde::Deserialize)]
            struct LeftEvent {
                player_id: i32,
            }
            
            if let Ok(left_event) = serde_json::from_str::<LeftEvent>(event.payload()) {
                let mut field_guard = field_for_left.lock().unwrap();
                if let Some(field) = field_guard.as_mut() {
                    field.remove_player(left_event.player_id);
                }
            }
        });

        // Подписываемся на событие player-steered для обработки управления
        app.listen("player-steered", move |event| {
            #[derive(serde::Deserialize)]
            struct SteerEvent {
                player_id: i32,
                direction: i32,
            }
            
            if let Ok(steer_event) = serde_json::from_str::<SteerEvent>(event.payload()) {
                let direction = crate::snakes::Direction::try_from(steer_event.direction);
                if let Ok(dir) = direction {
                    let mut steers_guard = pending_steers_for_steer.lock().unwrap();
                    steers_guard.insert(steer_event.player_id, dir);
                }
            }
        });
    }
}
