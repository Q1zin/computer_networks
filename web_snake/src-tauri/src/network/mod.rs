pub mod protocol;
pub mod udp_impl;

use tauri::Emitter;
use crate::game::state::{StateImpl, StateManager};
use crate::snakes::{
    game_message, game_message::RoleChangeMsg, GameAnnouncement, GameConfig, GameMessage, GamePlayer,
    GamePlayers, GameState, NodeRole, PlayerType,
};
use anyhow::{anyhow, Result};
use protocol::NetworkProtocol;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use udp_impl::UdpNetwork;

const GAME_TIMEOUT_SECS: u64 = 5;
const MULTICAST_ADDR: &str = "239.192.0.4:9192";

/// Неподтверждённое сообщение, ожидающее ACK
#[derive(Clone, Debug)]
struct PendingMessage {
    msg: GameMessage,
    dest_addr: SocketAddr,
    sent_at: Instant,
    retries: u32,
}

#[derive(Clone, Debug)]
struct DiscoveredGame {
    announcement: GameAnnouncement,
    master_address: SocketAddr,
    last_seen: Instant,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredGameDto {
    pub game_name: String,
    pub players_count: usize,
    pub can_join: bool,
    pub width: i32,
    pub height: i32,
    pub master_address: String,
    pub master_ip: Option<String>,
    pub master_port: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamesDiscoveredDto {
    pub games: Vec<DiscoveredGameDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordDto {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnakeDto {
    pub player_id: i32,
    pub points: Vec<CoordDto>,
    pub state: i32,
    pub head_direction: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePlayerDto {
    pub name: String,
    pub id: i32,
    pub ip_address: Option<String>,
    pub port: Option<i32>,
    pub role: i32,
    pub r#type: i32,
    pub score: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePlayersDto {
    pub players: Vec<GamePlayerDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameConfigDto {
    pub width: i32,
    pub height: i32,
    pub food_static: i32,
    pub state_delay_ms: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStateDto {
    pub state_order: i32,
    pub snakes: Vec<SnakeDto>,
    pub foods: Vec<CoordDto>,
    pub players: GamePlayersDto,
    pub config: GameConfigDto,
}

#[derive(Clone)]
pub struct NetworkService {
    net: Arc<UdpNetwork>,
    discovered: Arc<Mutex<Vec<DiscoveredGame>>>,
    current_master: Arc<Mutex<Option<SocketAddr>>>,
    last_state: Arc<Mutex<Option<GameState>>>,
    state_manager: Arc<Mutex<Box<dyn StateManager>>>,
    players: Arc<Mutex<GamePlayers>>,
    game_name: Arc<Mutex<String>>,
    game_config: Arc<Mutex<Option<GameConfig>>>,
    running: Arc<AtomicBool>,
    seq: Arc<Mutex<i64>>,
    /// Неподтверждённые сообщения, ожидающие ACK (msg_seq -> PendingMessage)
    pending_messages: Arc<Mutex<std::collections::HashMap<i64, PendingMessage>>>,
    /// Последний известный state_order (для игнорирования устаревших State)
    last_known_state_order: Arc<Mutex<i32>>,
    /// Флаг, указывающий что поле заполнено (нет места для новых змеек)
    is_full: Arc<AtomicBool>,
}

fn overlay_player_meta_from_players(state: &mut GameState, players: &GamePlayers) {
    use std::collections::HashMap;
    let by_id: HashMap<i32, &GamePlayer> = players.players.iter().map(|p| (p.id, p)).collect();

    // Важно: score хранится в игровом Field, его не перетираем.
    // А вот role/ip/port должен отражать топологию и реальные адреса.
    for p in &mut state.players.players {
        if let Some(src) = by_id.get(&p.id) {
            p.role = src.role;
            if src.ip_address.is_some() {
                p.ip_address = src.ip_address.clone();
            }
            if src.port.is_some() {
                p.port = src.port;
            }
        }
    }
}

impl NetworkService {
    pub fn new() -> Result<Self> {
        let net = UdpNetwork::new()?;
        Ok(Self {
            net: Arc::new(net),
            discovered: Arc::new(Mutex::new(Vec::new())),
            current_master: Arc::new(Mutex::new(None)),
            last_state: Arc::new(Mutex::new(None)),
            state_manager: Arc::new(Mutex::new(Box::new(StateImpl::new()))),
            players: Arc::new(Mutex::new(GamePlayers { players: vec![] })),
            game_name: Arc::new(Mutex::new(String::new())),
            game_config: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            seq: Arc::new(Mutex::new(0)),
            pending_messages: Arc::new(Mutex::new(std::collections::HashMap::new())),
            last_known_state_order: Arc::new(Mutex::new(0)),
            is_full: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn start_polling(&self, app: &AppHandle) {
        if self.running.swap(true, Ordering::SeqCst) {
            println!("Network polling already started");
            return;
        }

        let net = Arc::clone(&self.net);
        let discovered = Arc::clone(&self.discovered);
        let current_master = Arc::clone(&self.current_master);
        let last_state = Arc::clone(&self.last_state);
        let state_manager = Arc::clone(&self.state_manager);
        let players = Arc::clone(&self.players);
        let running = Arc::clone(&self.running);
        let game_name = Arc::clone(&self.game_name);
        let game_config = Arc::clone(&self.game_config);
        let seq = Arc::clone(&self.seq);
        let pending_messages = Arc::clone(&self.pending_messages);
        let last_known_state_order = Arc::clone(&self.last_known_state_order);
        let is_full = Arc::clone(&self.is_full);
        let app_handle = app.clone();

        std::thread::spawn(move || {
            let mut last_timeout_check = Instant::now();
            let mut last_ping_check = Instant::now();
            let mut last_announcement = Instant::now();
            let mut last_retransmit_check = Instant::now();
            
            while running.load(Ordering::SeqCst) {
                match net.poll_receive() {
                    Ok(Some((msg, addr))) => {
                        let is_game_message = matches!(
                            msg.r#type.as_ref(),
                            Some(game_message::Type::State(_))
                                | Some(game_message::Type::Ping(_))
                                | Some(game_message::Type::Ack(_))
                                | Some(game_message::Type::Join(_))
                                | Some(game_message::Type::Steer(_))
                                | Some(game_message::Type::RoleChange(_))
                                | Some(game_message::Type::Error(_))
                        );

                        if is_game_message {
                            use crate::game::state::GameMode;
                            let (is_master, current_mode) = {
                                let state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                                let mode = state_mgr.current_mode();
                                let is_m = matches!(
                                    mode,
                                    GameMode::InGame {
                                        role: NodeRole::Master,
                                        ..
                                    }
                                );
                                (is_m, format!("{:?}", mode))
                            };

                            if !is_master {
                                let master = current_master.lock().expect("master mutex poisoned");
                                // Если мы не в игре — игнорируем всё.
                                if master.is_none() {
                                    println!("[FILTER] Dropping message: current_master is None, mode={}", current_mode);
                                    continue;
                                }

                                // После смены мастера старый мастер должен перестать слать,
                                // а остальные — перестать принимать от старого мастера.
                                //
                                // Исключения (нужны для корректного switchover):
                                // - RoleChange принимаем всегда (может указать нового мастера)
                                // - State от нового мастера принимаем, если внутри state sender_id имеет роль MASTER
                                //   (это позволит обновить current_master даже если он ещё указывает на старого).
                                let current_master_addr = *master;
                                drop(master);

                                // Проверяем, является ли отправитель известным игроком
                                let sender_is_known_player = {
                                    let state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                                    state_mgr.get_known_players().iter().any(|(_, a)| *a == addr)
                                };

                                if let Some(cm) = current_master_addr {
                                    let allow = match msg.r#type.as_ref() {
                                        // RoleChange всегда принимаем - может быть от нового Master
                                        Some(game_message::Type::RoleChange(_)) => true,
                                        Some(game_message::Type::State(state_msg)) => {
                                            let sender_id = msg.sender_id.unwrap_or(0);
                                            // Принимаем State от текущего мастера по адресу (обычный режим),
                                            // либо при switchover — от нового мастера, если внутри State sender_id помечен MASTER.
                                            // Иначе получим ситуацию: current_master уже указывает на deputy, но State от него
                                            // будет отбрасываться из-за "старых" ролей в снапшоте.
                                            addr == cm
                                                || state_msg
                                                    .state
                                                    .players
                                                    .players
                                                    .iter()
                                                    .any(|p| {
                                                        p.id == sender_id
                                                            && p.role == NodeRole::Master as i32
                                                    })
                                        }
                                        // Ping/Ack от известных игроков принимаем - это важно для failover
                                        // когда Deputy становится Master и начинает слать до обновления current_master
                                        Some(game_message::Type::Ping(_)) | Some(game_message::Type::Ack(_)) => {
                                            addr == cm || sender_is_known_player
                                        }
                                        _ => addr == cm,
                                    };

                                    if !allow {
                                        println!("[FILTER] Dropping message from {}: not from current_master {:?}", addr, cm);
                                        continue;
                                    }
                                }
                            }
                        }

                        println!("Received message from {}: {:?}", addr, msg);
                        process_message(
                            &app_handle,
                            &discovered,
                            &current_master,
                            &last_state,
                            &state_manager,
                            &players,
                            &game_config,
                            &net,
                            &game_name,
                            &seq,
                            &pending_messages,
                            &last_known_state_order,
                            &is_full,
                            msg,
                            addr,
                        );
                    }
                    Ok(None) => {
                        // std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(err) => {
                        eprintln!("Network receive error: {err:#}");
                        // std::thread::sleep(Duration::from_millis(50));
                    }
                }

                // Все интервалы таймеров привязываем к state_delay_ms.
                let delay_ms = {
                    let cfg_guard = game_config.lock().expect("game_config mutex poisoned");
                    cfg_guard
                        .as_ref()
                        .and_then(|c| c.state_delay_ms)
                        .map(|v| v.max(1) as u64)
                        .unwrap_or(1000)
                };

                // Периодически проверяем таймауты (~0.2 * state_delay_ms)
                let timeout_check_interval = Duration::from_millis(((delay_ms * 2) / 10).max(1));
                if last_timeout_check.elapsed() > timeout_check_interval {
                    let delay_ms_u32 = delay_ms.min(u32::MAX as u64) as u32;

                    let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                    let mut players_guard = players.lock().expect("players mutex poisoned");
                    
                    if let Err(e) = state_mgr.check_timeouts(delay_ms_u32, &*net, &mut players_guard, &app_handle) {
                        eprintln!("Error checking timeouts: {}", e);
                    }

                    // Синхронизируем current_master с state machine, чтобы send_steer шёл на deputy
                    // после фейловера (а не на старый current_master).
                    {
                        use crate::game::state::GameMode;
                        let mut master_guard = current_master.lock().expect("master mutex poisoned");
                        let old_master = *master_guard;
                        let new_master = match state_mgr.current_mode() {
                            GameMode::InGame { role: NodeRole::Master, .. } => {
                                None
                            }
                            GameMode::InGame { master_addr: Some(addr), .. } => {
                                Some(addr)
                            }
                            _ => old_master,
                        };
                        
                        // Если мастер сменился — обновляем адреса в pending_messages
                        if old_master != new_master {
                            if let Some(new_addr) = new_master {
                                let mut pending = pending_messages.lock().expect("pending_messages mutex poisoned");
                                for pm in pending.values_mut() {
                                    if Some(pm.dest_addr) == old_master {
                                        println!("[MASTER_CHANGE] Redirecting pending msg_seq={} from {:?} to {}", 
                                            pm.msg.msg_seq, old_master, new_addr);
                                        pm.dest_addr = new_addr;
                                    }
                                }
                            }
                        }
                        
                        *master_guard = new_master;
                    }
                    
                    drop(players_guard);
                    drop(state_mgr);
                    last_timeout_check = Instant::now();
                }

                // Периодически отправляем пинги (~0.2 * state_delay_ms, а сам интервал пинга — 0.5*delay в StateManager)
                let ping_check_interval = Duration::from_millis(((delay_ms * 2) / 10).max(20));
                if last_ping_check.elapsed() > ping_check_interval {
                    let delay_ms_u32 = delay_ms.min(u32::MAX as u64) as u32;

                    let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                    
                    if let Err(e) = state_mgr.send_ping_if_needed(delay_ms_u32, &*net) {
                        eprintln!("Error sending pings: {}", e);
                    }
                    
                    drop(state_mgr);
                    last_ping_check = Instant::now();
                }

                // Периодически отправляем announcements (ТЗ: фиксированный интервал 1 секунда)
                let announcement_interval = Duration::from_secs(1);
                if last_announcement.elapsed() > announcement_interval {
                    use crate::game::state::GameMode;
                    
                    let state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                    let is_master = matches!(state_mgr.current_mode(), GameMode::InGame { role: NodeRole::Master, .. });
                    drop(state_mgr);
                    
                    if is_master {
                        let name_guard = game_name.lock().expect("game_name mutex poisoned");
                        let game_name_str = name_guard.clone();
                        drop(name_guard);
                        
                        if !game_name_str.is_empty() {
                            let config_guard = game_config.lock().expect("game_config mutex poisoned");
                            if let Some(config) = config_guard.as_ref() {
                                let cfg = *config;
                                drop(config_guard);
                                
                                // Отправляем announcement
                                let local_addr = net.get_local_addr().ok();
                                if let Some(addr) = local_addr {
                                    let players_guard = players.lock().expect("players mutex poisoned");
                                    let mut players_clone = players_guard.clone();
                                    
                                    // Заполняем IP и порт для Master
                                    for p in &mut players_clone.players {
                                        if p.role == NodeRole::Master as i32 {
                                            let ip = if addr.ip().is_unspecified() {
                                                "127.0.0.1".to_string()
                                            } else {
                                                addr.ip().to_string()
                                            };
                                            p.ip_address = Some(ip);
                                            p.port = Some(addr.port() as i32);
                                        }
                                    }
                                    drop(players_guard);
                                    
                                    // Фильтруем активных игроков
                                    let active_players = GamePlayers {
                                        players: players_clone.players
                                            .into_iter()
                                            .filter(|p| p.role != NodeRole::Viewer as i32)
                                            .collect(),
                                    };
                                    
                                    let announcement = crate::snakes::GameAnnouncement {
                                        players: active_players,
                                        config: cfg,
                                        can_join: Some(!is_full.load(Ordering::SeqCst)),
                                        game_name: game_name_str,
                                    };
                                    
                                    let msg_seq = {
                                        let mut seq_guard = seq.lock().expect("seq mutex poisoned");
                                        *seq_guard += 1;
                                        *seq_guard
                                    };
                                    
                                    let msg = crate::snakes::GameMessage {
                                        msg_seq,
                                        sender_id: None,
                                        receiver_id: None,
                                        r#type: Some(crate::snakes::game_message::Type::Announcement(
                                            crate::snakes::game_message::AnnouncementMsg {
                                                games: vec![announcement],
                                            }
                                        )),
                                    };
                                    
                                    let _ = net.send_multicast(msg);
                                    println!("Sent announcement");
                                }
                            }
                        }
                    }
                    
                    last_announcement = Instant::now();
                }

                // Периодически переотправляем неподтверждённые сообщения (каждые state_delay_ms / 10)
                let retransmit_interval = Duration::from_millis((delay_ms / 10).max(50));
                if last_retransmit_check.elapsed() > retransmit_interval {
                    let now = Instant::now();
                    let mut pending = pending_messages.lock().expect("pending_messages mutex poisoned");
                    
                    // Собираем сообщения для переотправки
                    let to_retransmit: Vec<(i64, GameMessage, SocketAddr)> = pending
                        .iter()
                        .filter(|(_, pm)| now.duration_since(pm.sent_at) > retransmit_interval)
                        .map(|(seq, pm)| (*seq, pm.msg.clone(), pm.dest_addr))
                        .collect();
                    
                    for (msg_seq, msg, dest_addr) in to_retransmit {
                        // Обновляем время отправки и счётчик
                        if let Some(pm) = pending.get_mut(&msg_seq) {
                            pm.sent_at = now;
                            pm.retries += 1;
                            
                            // После 10 попыток считаем узел недоступным и удаляем сообщение
                            if pm.retries > 10 {
                                println!("[RETRANSMIT] Giving up on msg_seq={} after {} retries", msg_seq, pm.retries);
                                pending.remove(&msg_seq);
                                continue;
                            }
                            
                            println!("[RETRANSMIT] Resending msg_seq={} to {} (retry #{})", msg_seq, dest_addr, pm.retries);
                            if let Err(e) = net.send_unicast(dest_addr, msg) {
                                eprintln!("[RETRANSMIT] Failed to resend: {}", e);
                            }
                        }
                    }
                    
                    drop(pending);
                    last_retransmit_check = Instant::now();
                }
            }
        });
    }

    pub fn send_discover(&self) -> Result<()> {
        let discover_msg = GameMessage {
            msg_seq: 0,
            sender_id: None,
            receiver_id: None,
            r#type: Some(game_message::Type::Discover(game_message::DiscoverMsg {})),
        };

        let multicast_addr: SocketAddr = MULTICAST_ADDR
            .parse::<SocketAddr>()
            .map_err(|e| anyhow!(e))?;
        self.net.send_unicast(multicast_addr, discover_msg)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_games(&self) -> Vec<DiscoveredGameDto> {
        let games = self.discovered.lock().expect("discovered mutex poisoned");
        games
            .iter()
            .map(|game| {
                // Находим мастера в списке игроков
                let master = game.announcement.players.players.iter()
                    .find(|p| p.role == NodeRole::Master as i32);
                
                // Извлекаем конфиг игры (width, height)
                let width = game.announcement.config.width.unwrap_or(40);
                let height = game.announcement.config.height.unwrap_or(30);
                
                DiscoveredGameDto {
                    game_name: game.announcement.game_name.clone(),
                    players_count: game.announcement.players.players.len(),
                    can_join: game.announcement.can_join.unwrap_or(true),
                    width,
                    height,
                    master_address: game.master_address.to_string(),
                    master_ip: master.and_then(|m| m.ip_address.clone()),
                    master_port: master.and_then(|m| m.port),
                }
            })
            .collect()
    }

    pub fn join_game(
        &self,
        game_name: &str,
        player_name: &str,
        requested_role: NodeRole,
    ) -> Result<()> {
        // Защита от "восстановления" старого State в UI: перед входом
        // сбрасываем локальные кеши предыдущей сессии.
        self.reset_local_session()?;

        // Берём мастер-адрес и конфиг из discovery (нужно для таймаутов 0.8*state_delay_ms)
        let (master_addr, discovered_config, discovered_players) = {
            let games = self.discovered.lock().expect("discovered mutex poisoned");
            let found = games
                .iter()
                .find(|g| g.announcement.game_name == game_name)
                .ok_or_else(|| anyhow!("master address for game '{game_name}' not found"))?;
            (found.master_address, found.announcement.config, found.announcement.players.clone())
        };

        *self.game_name.lock().expect("game_name mutex poisoned") = game_name.to_string();
        *self.game_config.lock().expect("game_config mutex poisoned") = Some(discovered_config);
        *self.players.lock().expect("players mutex poisoned") = discovered_players;

        let msg_seq = next_seq(&self.seq);
        let join_msg = GameMessage {
            msg_seq,
            sender_id: None,
            receiver_id: None,
            r#type: Some(game_message::Type::Join(game_message::JoinMsg {
                player_type: Some(PlayerType::Human as i32),
                player_name: player_name.to_string(),
                game_name: game_name.to_string(),
                requested_role: requested_role as i32,
            })),
        };

        // Сохраняем в pending для возможной переотправки
        self.add_pending_message(msg_seq, join_msg.clone(), master_addr);
        
        self.net.send_unicast(master_addr, join_msg)?;
        *self.current_master.lock().expect("master mutex poisoned") = Some(master_addr);
        self.net.set_role(requested_role);

        // Включаем state machine в InGame, иначе таймауты/пинги/фейловер работают неправильно.
        {
            use crate::game::state::GameMode;
            let deputy_id = self
                .players
                .lock()
                .expect("players mutex poisoned")
                .players
                .iter()
                .find(|p| p.role == NodeRole::Deputy as i32)
                .map(|p| p.id);

            let mut state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
            state_mgr.transition(
                GameMode::InGame {
                    role: requested_role,
                    master_addr: Some(master_addr),
                    deputy_id,
                },
                &*self.net,
            )?;
        }

        Ok(())
    }

    pub fn send_steer(&self, direction: i32) -> Result<()> {
        let master_addr = {
            if let Some(addr) = *self.current_master.lock().expect("master mutex poisoned") {
                addr
            } else {
                use crate::game::state::GameMode;
                let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
                match state_mgr.current_mode() {
                    GameMode::InGame { role: NodeRole::Master, .. } => {
                        return Err(anyhow!("master cannot send steer"));
                    }
                    GameMode::InGame { master_addr: Some(addr), .. } => addr,
                    _ => return Err(anyhow!("master address is not set")),
                }
            }
        };

        let msg_seq = next_seq(&self.seq);
        let sender_id = self.my_player_id();
        let msg = GameMessage {
            msg_seq,
            sender_id: Some(sender_id),
            receiver_id: None,
            r#type: Some(game_message::Type::Steer(game_message::SteerMsg { direction })),
        };

        // Сохраняем в pending для возможной переотправки
        self.add_pending_message(msg_seq, msg.clone(), master_addr);
        
        self.net.send_unicast(master_addr, msg)?;
        Ok(())
    }

    /// Добавляет сообщение в очередь ожидания ACK
    fn add_pending_message(&self, msg_seq: i64, msg: GameMessage, dest_addr: SocketAddr) {
        let mut pending = self.pending_messages.lock().expect("pending_messages mutex poisoned");
        pending.insert(msg_seq, PendingMessage {
            msg,
            dest_addr,
            sent_at: Instant::now(),
            retries: 0,
        });
    }

    pub fn leave_game(&self) -> Result<()> {
        // Сообщаем Master, что мы ВЫХОДИМ ИЗ ИГРЫ полностью (не просто становимся VIEWER-ом).
        // Отличаем от become_spectator() тем, что receiver_role = VIEWER.
        // Master на это удалит нас из known_players и перестанет слать State сразу.
        let master_addr = *self
            .current_master
            .lock()
            .expect("master mutex poisoned");

        let master_id = self
            .players
            .lock()
            .expect("players mutex poisoned")
            .players
            .iter()
            .find(|p| p.role == NodeRole::Master as i32)
            .map(|p| p.id);

        if let Some(master_addr) = master_addr {
            let sender_id = self.my_player_id();

            let msg_seq = next_seq(&self.seq);
            let msg = GameMessage {
                msg_seq,
                sender_id: Some(sender_id),
                receiver_id: master_id,
                r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                    sender_role: Some(NodeRole::Viewer as i32),
                    receiver_role: Some(NodeRole::Viewer as i32),
                })),
            };
            let _ = self.net.send_unicast(master_addr, msg);
        }

        // Локально сбрасываем состояние в Lobby: перестаём пинговать и обнуляем топологию.
        self.reset_local_session()?;

        // После reset_local_session мы уже в Lobby и current_master сброшен.
        // Роль оставляем VIEWER, чтобы не слать игровые сообщения.
        Ok(())
    }

    /// Полный локальный сброс сессии (без остановки polling-треда).
    /// Нужен, чтобы новая игра не «подхватывала» кеши старой (last_state/players/my_id/etc.).
    pub fn reset_local_session(&self) -> Result<()> {
        use crate::game::state::GameMode;

        // Сбрасываем state machine
        {
            let mut state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
            state_mgr.transition(GameMode::Lobby, &*self.net)?;
        }

        // Сбрасываем ссылки на мастера
        *self.current_master.lock().expect("master mutex poisoned") = None;

        // Очищаем локальные кеши, которые используются UI (иначе get_game_state вернёт старое)
        *self.last_state.lock().expect("state mutex poisoned") = None;
        self.players.lock().expect("players mutex poisoned").players.clear();
        *self.game_name.lock().expect("game_name mutex poisoned") = String::new();
        *self.game_config.lock().expect("game_config mutex poisoned") = None;

        // Сбрасываем pending messages и state_order
        self.pending_messages.lock().expect("pending_messages mutex poisoned").clear();
        *self.last_known_state_order.lock().expect("last_known_state_order mutex poisoned") = 0;

        // Переходим в роль VIEWER по умолчанию
        self.net.set_role(NodeRole::Viewer);
        Ok(())
    }

    pub fn become_spectator(&self) -> Result<()> {
        use crate::game::state::GameMode;

        // Если мы сейчас MASTER и хотим стать VIEWER — делаем явный handoff на deputy.
        // Иначе deputy/normal будут ждать таймаут 0.8*delay.
        {
            let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
            if let GameMode::InGame { role: NodeRole::Master, deputy_id, .. } = state_mgr.current_mode() {
                drop(state_mgr);

                let deputy_id = deputy_id.or_else(|| {
                    self.players
                        .lock()
                        .expect("players mutex poisoned")
                        .players
                        .iter()
                        .find(|p| p.role == NodeRole::Deputy as i32)
                        .map(|p| p.id)
                });

                let Some(deputy_id) = deputy_id else {
                    // Нет deputy (например, мы одни). Просто становимся VIEWER.
                    let mut state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
                    state_mgr.become_viewer(None, None, &*self.net)?;
                    *self.current_master.lock().expect("master mutex poisoned") = None;
                    return Ok(());
                };

                // Адрес deputy берём из state_manager known_players (или из players ip/port, если есть).
                let deputy_addr = {
                    let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
                    state_mgr
                        .get_known_players()
                        .into_iter()
                        .find(|(id, _)| *id == deputy_id)
                        .map(|(_, addr)| addr)
                }
                .or_else(|| {
                    self.players
                        .lock()
                        .expect("players mutex poisoned")
                        .players
                        .iter()
                        .find(|p| p.id == deputy_id)
                        .and_then(|p| {
                            let ip = p.ip_address.as_ref()?;
                            let port = p.port?;
                            format!("{}:{}", ip, port).parse::<SocketAddr>().ok()
                        })
                })
                .ok_or_else(|| anyhow!("deputy address is not known"))?;

                // Сообщаем deputy: "ты теперь MASTER" (receiver_role=MASTER).
                // Важно: после того как мы перестали быть MASTER, мы не должны слать вообще ничего.
                // Поэтому сообщение отправляем ДО переключения в VIEWER.
                let sender_id = self.my_player_id();
                let msg_seq = next_seq(&self.seq);
                let msg = GameMessage {
                    msg_seq,
                    sender_id: Some(sender_id),
                    receiver_id: Some(deputy_id),
                    r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                        sender_role: Some(NodeRole::Master as i32),
                        receiver_role: Some(NodeRole::Master as i32),
                    })),
                };
                let _ = self.net.send_unicast(deputy_addr, msg);

                // Локально становимся VIEWER и переключаем current_master на deputy.
                {
                    let mut state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
                    state_mgr.become_viewer(Some(deputy_addr), Some(deputy_id), &*self.net)?;
                }
                *self.current_master.lock().expect("master mutex poisoned") = Some(deputy_addr);
                return Ok(());
            }
        }

        let master_addr = {
            if let Some(addr) = *self.current_master.lock().expect("master mutex poisoned") {
                addr
            } else {
                let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
                match state_mgr.current_mode() {
                    GameMode::InGame { master_addr: Some(addr), .. } => addr,
                    _ => return Err(anyhow!("master address is not set")),
                }
            }
        };

        let msg_seq = next_seq(&self.seq);

        let master_id = self
            .players
            .lock()
            .expect("players mutex poisoned")
            .players
            .iter()
            .find(|p| p.role == NodeRole::Master as i32)
            .map(|p| p.id);

        let msg = GameMessage {
            msg_seq,
            sender_id: Some(self.my_player_id()),
            receiver_id: master_id,
            r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                sender_role: Some(NodeRole::Viewer as i32),
                receiver_role: None,
            })),
        };

        self.net.send_unicast(master_addr, msg)?;
        self.net.set_role(NodeRole::Viewer);
        Ok(())
    }

    pub fn latest_state(&self) -> Option<GameStateDto> {
        let state = self.last_state.lock().expect("state mutex poisoned");
        let config_guard = self.game_config.lock().expect("game_config mutex poisoned");
        let config = config_guard.as_ref().cloned().unwrap_or(GameConfig {
            width: Some(40),
            height: Some(30),
            food_static: Some(1),
            state_delay_ms: Some(1000),
        });
        state.as_ref().map(|s| game_state_to_dto(s, &config))
    }

    /// Последний сырой GameState, полученный от мастера (для восстановления симуляции на deputy).
    pub fn latest_state_raw(&self) -> Option<GameState> {
        self.last_state
            .lock()
            .expect("state mutex poisoned")
            .as_ref()
            .cloned()
    }

    /// Текущий GameConfig игры, сохранённый из announcement/join.
    pub fn current_game_config_raw(&self) -> Option<GameConfig> {
        self.game_config
            .lock()
            .expect("game_config mutex poisoned")
            .as_ref()
            .cloned()
    }

    pub fn init_as_master(&self, game_name: String, config: GameConfig, initial_player: GamePlayer) -> Result<()> {
        use crate::game::state::GameMode;
        
        *self.game_name.lock().expect("game_name mutex poisoned") = game_name;
        *self.game_config.lock().expect("game_config mutex poisoned") = Some(config);
        
        let mut players_guard = self.players.lock().expect("players mutex poisoned");
        players_guard.players = vec![initial_player];
        drop(players_guard);

        let mut state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
        state_mgr.transition(
            GameMode::InGame {
                role: NodeRole::Master,
                master_addr: None,
                deputy_id: None,
            },
            &*self.net,
        )?;
        drop(state_mgr);

        self.net.set_role(NodeRole::Master);
        Ok(())
    }

    /// Отправка State всем известным игрокам (только для Master)
    /// Отправляет всем игрокам в known_players (включая зрителей VIEWER)
    /// Вышедшие игроки и игроки с timeout уже удалены из known_players
    pub fn broadcast_state(&self, state: &GameState) -> Result<()> {
        use crate::game::state::GameMode;

        // Жёсткое правило: если узел перестал быть MASTER — он не должен слать вообще ничего.
        // (в т.ч. если ещё работает локальный GameManager loop)
        {
            let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
            let is_master = matches!(
                state_mgr.current_mode(),
                GameMode::InGame {
                    role: NodeRole::Master,
                    ..
                }
            );
            if !is_master {
                return Ok(());
            }
        }

        let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
        let known_players = state_mgr.get_known_players();
        drop(state_mgr);

        // Важно: роли (в т.ч. DEPUTY) определяются сетевым state_manager/players,
        // а не игровым Field. Поэтому перед рассылкой накладываем meta (role/ip/port) на State.
        let mut state_to_send = state.clone();
        let players_snapshot = self.players.lock().expect("players mutex poisoned").clone();
        overlay_player_meta_from_players(&mut state_to_send, &players_snapshot);

        // КРИТИЧНО для deputy takeover: Master должен включать ip/port ВСЕХ игроков в State,
        // иначе deputy при takeover не знает куда слать State и RoleChange.
        // Берём адреса из known_players и добавляем в State.
        for (player_id, addr) in &known_players {
            for p in &mut state_to_send.players.players {
                if p.id == *player_id {
                    if p.ip_address.is_none() {
                        p.ip_address = Some(addr.ip().to_string());
                    }
                    if p.port.is_none() {
                        p.port = Some(addr.port() as i32);
                    }
                    break;
                }
            }
        }

        // Дополнительно: собственный ip/port Master'а (он сам себе сообщений не шлёт, поэтому
        // его нет в known_players как отправителя).
        if let Ok(local_addr) = self.net.get_local_addr() {
            let ip = if local_addr.ip().is_unspecified() {
                "127.0.0.1".to_string()
            } else {
                local_addr.ip().to_string()
            };
            for p in &mut state_to_send.players.players {
                if p.role == NodeRole::Master as i32 {
                    if p.ip_address.is_none() {
                        p.ip_address = Some(ip.clone());
                    }
                    if p.port.is_none() {
                        p.port = Some(local_addr.port() as i32);
                    }
                }
            }
        }

        let msg_seq = next_seq(&self.seq);
        let msg = GameMessage {
            msg_seq,
            sender_id: Some(self.my_player_id()),
            receiver_id: None,
            r#type: Some(game_message::Type::State(game_message::StateMsg {
                state: state_to_send,
            })),
        };

        // Отправляем всем игрокам в known_players (вышедшие уже удалены)
        let my_id = self.my_player_id();
        for (player_id, addr) in known_players {
            if player_id != my_id {
                if let Err(e) = self.net.send_unicast(addr, msg.clone()) {
                    eprintln!("Failed to send state to player {}: {}", player_id, e);
                }
            }
        }

        Ok(())
    }

    pub fn get_players(&self) -> GamePlayers {
        let players_guard = self.players.lock().expect("players mutex poisoned");
        players_guard.clone()
    }

    pub fn my_player_id(&self) -> i32 {
        let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
        state_mgr.my_id()
    }

    #[allow(dead_code)]
    pub fn send_announcement(&self, game_name: String, config: &GameConfig, is_full: bool) -> Result<()> {
        use crate::snakes::{GameAnnouncement, game_message::AnnouncementMsg};
        
        let local_addr = self.net.get_local_addr()?;
        let mut players = self.players.lock().expect("players mutex poisoned").clone();
        
        // Заполняем IP и порт для Master в announcement
        for p in &mut players.players {
            if p.role == NodeRole::Master as i32 {
                let ip = if local_addr.ip().is_unspecified() {
                    "127.0.0.1".to_string()
                } else {
                    local_addr.ip().to_string()
                };
                p.ip_address = Some(ip);
                p.port = Some(local_addr.port() as i32);
            }
        }

        // Фильтруем только активных игроков (не Viewer)
        let active_players = GamePlayers {
            players: players.players
                .into_iter()
                .filter(|p| p.role != NodeRole::Viewer as i32)
                .collect(),
        };

        let announcement = GameAnnouncement {
            players: active_players,
            config: *config,
            can_join: Some(!is_full),
            game_name,
        };

        let msg_seq = next_seq(&self.seq);
        let msg = GameMessage {
            msg_seq,
            sender_id: None,
            receiver_id: None,
            r#type: Some(game_message::Type::Announcement(AnnouncementMsg {
                games: vec![announcement],
            })),
        };

        self.net.send_multicast(msg)?;
        println!("Sent announcement");
        Ok(())
    }

    /// Установить флаг заполненности поля (вызывается из GameManager)
    pub fn set_is_full(&self, full: bool) {
        self.is_full.store(full, Ordering::SeqCst);
    }

    /// Получить текущее состояние заполненности поля
    #[allow(dead_code)]
    pub fn get_is_full(&self) -> bool {
        self.is_full.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    fn find_master_addr(&self, game_name: &str) -> Result<SocketAddr> {
        let games = self.discovered.lock().expect("discovered mutex poisoned");
        let found = games
            .iter()
            .find(|g| g.announcement.game_name == game_name)
            .map(|g| g.master_address);

        found.ok_or_else(|| anyhow!("master address for game '{game_name}' not found"))
    }
}

fn process_message(
    app: &AppHandle,
    discovered: &Arc<Mutex<Vec<DiscoveredGame>>>,
    current_master: &Arc<Mutex<Option<SocketAddr>>>,
    last_state: &Arc<Mutex<Option<GameState>>>,
    state_manager: &Arc<Mutex<Box<dyn StateManager>>>,
    players: &Arc<Mutex<GamePlayers>>,
    game_config: &Arc<Mutex<Option<GameConfig>>>,
    net: &Arc<UdpNetwork>,
    game_name: &Arc<Mutex<String>>,
    seq: &Arc<Mutex<i64>>,
    pending_messages: &Arc<Mutex<std::collections::HashMap<i64, PendingMessage>>>,
    last_known_state_order: &Arc<Mutex<i32>>,
    is_full: &Arc<AtomicBool>,
    msg: GameMessage,
    addr: SocketAddr,
) {
    let now = Instant::now();

    // Обрабатываем ACK: удаляем соответствующее сообщение из pending
    if let Some(game_message::Type::Ack(_)) = msg.r#type.as_ref() {
        let mut pending = pending_messages.lock().expect("pending_messages mutex poisoned");
        if pending.remove(&msg.msg_seq).is_some() {
            println!("[ACK] Received ACK for msg_seq={}, removed from pending", msg.msg_seq);
        }
    }

    // Сначала обрабатываем Announcement и Discover (не требуют StateManager)
    match msg.r#type.as_ref() {
        Some(game_message::Type::Announcement(announcement_msg)) => {
            let mut games = discovered.lock().expect("discovered mutex poisoned");
            games.retain(|game| now.duration_since(game.last_seen) < Duration::from_secs(GAME_TIMEOUT_SECS));

            for announcement in &announcement_msg.games {
                let master_from_payload = announcement
                    .players
                    .players
                    .iter()
                    .find(|p| p.role == NodeRole::Master as i32)
                    .and_then(|p| {
                        let ip = p.ip_address.as_ref()?;
                        let port = p.port?;
                        format!("{}:{}", ip, port).parse::<SocketAddr>().ok()
                    });

                let master_addr = master_from_payload.unwrap_or(addr);

                if let Some(existing) = games
                    .iter_mut()
                    .find(|g| g.announcement.game_name == announcement.game_name)
                {
                    existing.announcement = announcement.clone();
                    existing.master_address = master_addr;
                    existing.last_seen = now;
                } else {
                    games.push(DiscoveredGame {
                        announcement: announcement.clone(),
                        master_address: master_addr,
                        last_seen: now,
                    });
                }
            }

            let payload = GamesDiscoveredDto {
                games: games
                    .iter()
                    .map(|game| {
                        // Находим мастера в списке игроков
                        let master = game.announcement.players.players.iter()
                            .find(|p| p.role == NodeRole::Master as i32);
                        
                        // Извлекаем конфиг игры (width, height)
                        let width = game.announcement.config.width.unwrap_or(40);
                        let height = game.announcement.config.height.unwrap_or(30);
                        
                        DiscoveredGameDto {
                            game_name: game.announcement.game_name.clone(),
                            players_count: game.announcement.players.players.len(),
                            can_join: game.announcement.can_join.unwrap_or(true),
                            width,
                            height,
                            master_address: game.master_address.to_string(),
                            master_ip: master.and_then(|m| m.ip_address.clone()),
                            master_port: master.and_then(|m| m.port),
                        }
                    })
                    .collect(),
            };
            let _ = app.emit("games-discovered", payload);
            return;
        }
        Some(game_message::Type::Discover(_)) => {
            let _ = app.emit("network-event", "discover");
            
            // Если мы Master - сразу отправляем Announcement в ответ
            use crate::game::state::GameMode;
            let state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
            let is_master = matches!(state_mgr.current_mode(), GameMode::InGame { role: NodeRole::Master, .. });
            drop(state_mgr);
            
            if is_master {
                let name_guard = game_name.lock().expect("game_name mutex poisoned");
                let game_name_str = name_guard.clone();
                drop(name_guard);
                
                if !game_name_str.is_empty() {
                    let config_guard = game_config.lock().expect("game_config mutex poisoned");
                    if let Some(config) = config_guard.as_ref() {
                        let cfg = *config;
                        drop(config_guard);
                        
                        let local_addr = net.get_local_addr().ok();
                        if let Some(addr) = local_addr {
                            let players_guard = players.lock().expect("players mutex poisoned");
                            let mut players_clone = players_guard.clone();
                            
                            // Заполняем IP и порт для Master
                            for p in &mut players_clone.players {
                                if p.role == NodeRole::Master as i32 {
                                    let ip = if addr.ip().is_unspecified() {
                                        "127.0.0.1".to_string()
                                    } else {
                                        addr.ip().to_string()
                                    };
                                    p.ip_address = Some(ip);
                                    p.port = Some(addr.port() as i32);
                                }
                            }
                            drop(players_guard);
                            
                            // Фильтруем активных игроков
                            let active_players = GamePlayers {
                                players: players_clone.players
                                    .into_iter()
                                    .filter(|p| p.role != NodeRole::Viewer as i32)
                                    .collect(),
                            };
                            
                            let announcement = crate::snakes::GameAnnouncement {
                                players: active_players,
                                config: cfg,
                                can_join: Some(!is_full.load(Ordering::SeqCst)),
                                game_name: game_name_str,
                            };
                            
                            let msg_seq = {
                                let mut seq_guard = seq.lock().expect("seq mutex poisoned");
                                *seq_guard += 1;
                                *seq_guard
                            };
                            
                            let msg = crate::snakes::GameMessage {
                                msg_seq,
                                sender_id: None,
                                receiver_id: None,
                                r#type: Some(crate::snakes::game_message::Type::Announcement(
                                    crate::snakes::game_message::AnnouncementMsg {
                                        games: vec![announcement],
                                    }
                                )),
                            };
                            
                            // По ТЗ: ответ на DiscoverMsg отправляется unicast отправителю
                            let _ = net.send_unicast(addr, msg);
                            println!("Sent announcement (unicast response to Discover from {})", addr);
                        }
                    }
                }
            }
            return;
        }
        _ => {}
    }

    // Обрабатываем State отдельно (для обновления UI)
    if let Some(game_message::Type::State(state_msg)) = msg.r#type.as_ref() {
        // Проверяем state_order: игнорируем устаревшие State
        let incoming_order = state_msg.state.state_order;
        {
            let mut known_order = last_known_state_order.lock().expect("last_known_state_order mutex poisoned");
            if incoming_order <= *known_order {
                println!("[State] Ignoring outdated state_order={} (known={})", incoming_order, *known_order);
                // Но всё равно нужно отправить ACK и обработать через StateManager для last_seen
                // Поэтому не делаем return, просто не обновляем UI
            } else {
                *known_order = incoming_order;
                
                // Обновляем last_state и players только для новых State
                let mut state_guard = last_state.lock().expect("state mutex poisoned");
                *state_guard = Some(state_msg.state.clone());
                drop(state_guard);
                
                let mut players_guard = players.lock().expect("players mutex poisoned");
                *players_guard = state_msg.state.players.clone();
                drop(players_guard);

                // Получаем config для передачи в DTO
                let config_guard = game_config.lock().expect("game_config mutex poisoned");
                let config = config_guard.as_ref().cloned().unwrap_or(GameConfig {
                    width: Some(40),
                    height: Some(30),
                    food_static: Some(1),
                    state_delay_ms: Some(1000),
                });
                drop(config_guard);

                let payload = game_state_to_dto(&state_msg.state, &config);
                let _ = app.emit("game-state", payload);
            }
        }

        // Если мы не MASTER, то адрес отправителя State считаем текущим мастером,
        // но только если этот State действительно пришёл от MASTER.
        // Это помогает NORMAL быстро переключаться на deputy после takeover.
        {
            use crate::game::state::GameMode;
            let state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
            let is_master = matches!(state_mgr.current_mode(), GameMode::InGame { role: NodeRole::Master, .. });
            let is_viewer = matches!(state_mgr.current_mode(), GameMode::InGame { role: NodeRole::Viewer, .. } | GameMode::Viewer);
            let my_id = state_mgr.my_id();
            drop(state_mgr);
            
            if !is_master {
                let sender_id = msg.sender_id.unwrap_or(0);
                let is_state_from_master = state_msg
                    .state
                    .players
                    .players
                    .iter()
                    .any(|p| p.id == sender_id && p.role == NodeRole::Master as i32);

                if is_state_from_master {
                    *current_master.lock().expect("master mutex poisoned") = Some(addr);
                }
                
                // Если мы не viewer и нашей змейки больше нет в State - становимся viewer
                // НО: проверяем, что мы уже были активным игроком (есть в players как не-Viewer),
                // иначе это может быть первый State после Join, когда змейка ещё не spawn'илась
                if !is_viewer && my_id != 0 {
                    let my_snake_alive = state_msg.state.snakes.iter().any(|s| s.player_id == my_id);
                    
                    // Проверяем, есть ли мы в players как активный игрок (не Viewer)
                    let am_i_active_player = state_msg
                        .state
                        .players
                        .players
                        .iter()
                        .any(|p| p.id == my_id && p.role != NodeRole::Viewer as i32);
                    
                    // Считаем "смертью" только если мы были активным игроком и змейки нет
                    if !my_snake_alive && am_i_active_player {
                        println!("[State] My snake (id={}) died, becoming viewer", my_id);
                        
                        // Отправляем RoleChange мастеру
                        let role_change = GameMessage {
                            msg_seq: 0, // будет заменён
                            sender_id: Some(my_id),
                            receiver_id: Some(sender_id),
                            r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                                sender_role: Some(NodeRole::Viewer as i32),
                                receiver_role: None, // просто становимся viewer, не выходим
                            })),
                        };
                        let _ = net.send_unicast(addr, role_change);
                        
                        // Переключаемся в режим viewer
                        let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                        let _ = state_mgr.become_viewer(Some(addr), None, &**net);
                    }
                }
            }
        }
    }

    // Все сообщения (в т.ч. State) обрабатываем через StateManager, чтобы:
    // - обновлять last_seen (таймаут мастера зависит от этого)
    // - слать ACK на State
    let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
    let mut players_guard = players.lock().expect("players mutex poisoned");

    // Подмешиваем адреса игроков из players (которые приходят в State) в known_players.
    // Это критично для deputy takeover: иначе deputy не знает куда слать State.
    state_mgr.observe_players(&players_guard);
    
    let is_full_val = is_full.load(Ordering::SeqCst);
    if let Err(e) = state_mgr.handle_message(msg.clone(), addr, &**net, &mut players_guard, app, is_full_val) {
        eprintln!("Error handling message: {}", e);
    }
    
    // Эмитим события для типов сообщений, которые нужно обработать в GameManager
    // Делаем это до drop, чтобы иметь доступ к state_mgr
    match msg.r#type.as_ref() {
        Some(game_message::Type::Join(join_msg)) => {
            // Join обрабатывается StateManager, который назначает ID
            // Находим игрока по имени в текущем списке игроков
            for player in &players_guard.players {
                if player.name == join_msg.player_name {
                    // Эмитим событие только для игроков (не для зрителей)
                    if player.role != NodeRole::Viewer as i32 {
                        #[derive(Clone, serde::Serialize)]
                        struct JoinEvent {
                            player_name: String,
                            player_id: i32,
                        }
                        let _ = app.emit("player-joined", JoinEvent {
                            player_name: player.name.clone(),
                            player_id: player.id,
                        });
                    }
                    break;
                }
            }
        }
        Some(game_message::Type::Steer(steer_msg)) => {
            // Эмитим событие для обработки в GameManager
            #[derive(Clone, serde::Serialize)]
            struct SteerEvent {
                player_id: i32,
                direction: i32,
            }
            if let Some(sender_id) = msg.sender_id {
                let _ = app.emit("player-steered", SteerEvent {
                    player_id: sender_id,
                    direction: steer_msg.direction,
                });
            }
        }
        Some(game_message::Type::Error(error_msg)) => {
            let _ = app.emit("game-error", error_msg.error_message.clone());
            
            // При получении ErrorMsg (например, нет места на поле) — сбрасываем сессию в Lobby,
            // чтобы клиент мог попробовать подключиться заново или к другой игре.
            // ВАЖНО: state_mgr уже захвачен выше (строка 1353), поэтому не блокируем снова!
            use crate::game::state::GameMode;
            let _ = state_mgr.transition(GameMode::Lobby, &**net);
            
            *current_master.lock().expect("current_master mutex poisoned") = None;
        }
        _ => {
            // Остальные события эмитим как общие network-event
            if let Some(msg_type) = msg.r#type.as_ref() {
                let event_name = match msg_type {
                    game_message::Type::Ping(_) => "ping",
                    game_message::Type::Ack(_) => "ack",
                    game_message::Type::RoleChange(_) => "role-change",
                    _ => return,
                };
                let _ = app.emit("network-event", event_name);
            }
        }
    }
    
    drop(players_guard);
    drop(state_mgr);
}

pub fn game_state_to_dto(state: &GameState, config: &GameConfig) -> GameStateDto {
    GameStateDto {
        state_order: state.state_order,
        snakes: state
            .snakes
            .iter()
            .map(|snake| SnakeDto {
                player_id: snake.player_id,
                points: snake
                    .points
                    .iter()
                    .map(|p| CoordDto {
                        x: p.x.unwrap_or(0),
                        y: p.y.unwrap_or(0),
                    })
                    .collect(),
                state: snake.state,
                head_direction: snake.head_direction,
            })
            .collect(),
        foods: state
            .foods
            .iter()
            .map(|food| CoordDto {
                x: food.x.unwrap_or(0),
                y: food.y.unwrap_or(0),
            })
            .collect(),
        players: GamePlayersDto {
            players: state
                .players
                .players
                .iter()
                .map(game_player_to_dto)
                .collect(),
        },
        config: GameConfigDto {
            width: config.width.unwrap_or(40),
            height: config.height.unwrap_or(30),
            food_static: config.food_static.unwrap_or(1),
            state_delay_ms: config.state_delay_ms.unwrap_or(1000),
        },
    }
}

fn game_player_to_dto(player: &GamePlayer) -> GamePlayerDto {
    GamePlayerDto {
        name: player.name.clone(),
        id: player.id,
        ip_address: player.ip_address.clone(),
        port: player.port,
        role: player.role,
        r#type: player.r#type.unwrap_or(PlayerType::Human as i32),
        score: player.score,
    }
}

fn next_seq(seq: &Arc<Mutex<i64>>) -> i64 {
    let mut guard = seq.lock().expect("seq mutex poisoned");
    *guard += 1;
    *guard
}
