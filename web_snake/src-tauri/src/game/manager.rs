use crate::game::field::{FieldImpl, GameField};
use crate::snakes::{Direction, GameConfig, GamePlayers, GameState, GamePlayer};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct GameManager {
    field: Arc<Mutex<Option<FieldImpl>>>,
    pending_steers: Arc<Mutex<HashMap<i32, Direction>>>,
    my_player_id: Arc<Mutex<Option<i32>>>,
    last_tick: Arc<Mutex<Instant>>,
}

impl GameManager {
    pub fn new() -> Self {
        Self {
            field: Arc::new(Mutex::new(None)),
            pending_steers: Arc::new(Mutex::new(HashMap::new())),
            my_player_id: Arc::new(Mutex::new(None)),
            last_tick: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Создание новой игры (для Master)
    pub fn create_game(&self, config: GameConfig, host_player: GamePlayer) -> Result<()> {
        let players = GamePlayers {
            players: vec![host_player],
        };
        
        let field = FieldImpl::new(config, players);
        *self.field.lock().unwrap() = Some(field);
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

    /// Обработка смерти змейки
    pub fn handle_death(&self, snake_id: i32) {
        let mut field_guard = self.field.lock().unwrap();
        if let Some(field) = field_guard.as_mut() {
            field.handle_death(snake_id);
        }
    }

    /// Сброс игры
    pub fn reset(&self) {
        *self.field.lock().unwrap() = None;
        self.pending_steers.lock().unwrap().clear();
        *self.my_player_id.lock().unwrap() = None;
        *self.last_tick.lock().unwrap() = Instant::now();
    }
}
