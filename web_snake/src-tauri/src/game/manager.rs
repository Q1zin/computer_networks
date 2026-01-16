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
                                
                                // Отправляем State через event используя DTO (для локального фронтенда)
                                let state_dto = game_state_to_dto(&new_state, &game_config);
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
                    }
                }

                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }

    /// Настройка обработчиков сетевых событий для Master
    pub fn setup_network_handlers(&self, app: AppHandle) {
        let field_for_join = Arc::clone(&self.field);
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
                let mut field_guard = field_for_left.lock().unwrap();
                if let Some(field) = field_guard.as_mut() {
                    field.change_snake_to_zombie(zombie_event.player_id);
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
