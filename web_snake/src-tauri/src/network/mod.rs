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
        })
    }

    pub fn start_polling(&self, app: &AppHandle) {
        if self.running.swap(true, Ordering::SeqCst) {
            println!("Network polling already started");
            return;
        }

        let net = Arc::clone(&self.net);
        let discovered = Arc::clone(&self.discovered);
        let last_state = Arc::clone(&self.last_state);
        let state_manager = Arc::clone(&self.state_manager);
        let players = Arc::clone(&self.players);
        let running = Arc::clone(&self.running);
        let game_name = Arc::clone(&self.game_name);
        let game_config = Arc::clone(&self.game_config);
        let seq = Arc::clone(&self.seq);
        let app_handle = app.clone();

        std::thread::spawn(move || {
            let mut last_timeout_check = Instant::now();
            let mut last_ping_check = Instant::now();
            let mut last_announcement = Instant::now();
            
            while running.load(Ordering::SeqCst) {
                match net.poll_receive() {
                    Ok(Some((msg, addr))) => {
                        println!("Received message from {}: {:?}", addr, msg);
                        process_message(
                            &app_handle,
                            &discovered,
                            &last_state,
                            &state_manager,
                            &players,
                            &game_config,
                            &net,
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

                // Периодически проверяем таймауты (каждые 200ms)
                if last_timeout_check.elapsed() > Duration::from_millis(200) {
                    let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                    let mut players_guard = players.lock().expect("players mutex poisoned");
                    
                    if let Err(e) = state_mgr.check_timeouts(1000, &*net, &mut players_guard) {
                        eprintln!("Error checking timeouts: {}", e);
                    }
                    
                    drop(players_guard);
                    drop(state_mgr);
                    last_timeout_check = Instant::now();
                }

                // Периодически отправляем пинги (каждые 300ms)
                if last_ping_check.elapsed() > Duration::from_millis(300) {
                    let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
                    
                    if let Err(e) = state_mgr.send_ping_if_needed(1000, &*net) {
                        eprintln!("Error sending pings: {}", e);
                    }
                    
                    drop(state_mgr);
                    last_ping_check = Instant::now();
                }

                // Периодически отправляем announcements (каждую секунду для Master)
                if last_announcement.elapsed() > Duration::from_secs(1) {
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
                                    let mut players_guard = players.lock().expect("players mutex poisoned");
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
                                        can_join: Some(true), // TODO: check is_full from GameManager
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

    pub fn list_games(&self) -> Vec<DiscoveredGameDto> {
        let games = self.discovered.lock().expect("discovered mutex poisoned");
        games
            .iter()
            .map(|game| {
                // Находим мастера в списке игроков
                let master = game.announcement.players.players.iter()
                    .find(|p| p.role == NodeRole::Master as i32);
                
                DiscoveredGameDto {
                    game_name: game.announcement.game_name.clone(),
                    players_count: game.announcement.players.players.len(),
                    can_join: game.announcement.can_join.unwrap_or(true),
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
        let master_addr = self.find_master_addr(game_name)?;

        let join_msg = GameMessage {
            msg_seq: 0,
            sender_id: None,
            receiver_id: None,
            r#type: Some(game_message::Type::Join(game_message::JoinMsg {
                player_type: Some(PlayerType::Human as i32),
                player_name: player_name.to_string(),
                game_name: game_name.to_string(),
                requested_role: requested_role as i32,
            })),
        };

        self.net.send_unicast(master_addr, join_msg)?;
        *self.current_master.lock().expect("master mutex poisoned") = Some(master_addr);
        self.net.set_role(requested_role);
        Ok(())
    }

    pub fn send_steer(&self, direction: i32) -> Result<()> {
        let master_addr = self
            .current_master
            .lock()
            .expect("master mutex poisoned")
            .ok_or_else(|| anyhow!("master address is not set"))?;

        let msg_seq = next_seq(&self.seq);
        let msg = GameMessage {
            msg_seq,
            sender_id: None,
            receiver_id: None,
            r#type: Some(game_message::Type::Steer(game_message::SteerMsg { direction })),
        };

        self.net.send_unicast(master_addr, msg)?;
        Ok(())
    }

    pub fn leave_game(&self) -> Result<()> {
        let master_addr = self
            .current_master
            .lock()
            .expect("master mutex poisoned")
            .ok_or_else(|| anyhow!("master address is not set"))?;

        let msg_seq = next_seq(&self.seq);
        let msg = GameMessage {
            msg_seq,
            sender_id: None,
            receiver_id: Some(1),
            r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                sender_role: Some(NodeRole::Viewer as i32),
                receiver_role: None,
            })),
        };

        self.net.send_unicast(master_addr, msg)?;
        self.net.set_role(NodeRole::Viewer);
        Ok(())
    }

    pub fn become_spectator(&self) -> Result<()> {
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
    pub fn broadcast_state(&self, state: &GameState) -> Result<()> {
        let state_mgr = self.state_manager.lock().expect("state_manager mutex poisoned");
        let known_players = state_mgr.get_known_players();
        drop(state_mgr);

        let msg_seq = next_seq(&self.seq);
        let msg = GameMessage {
            msg_seq,
            sender_id: Some(1), // Master ID
            receiver_id: None,
            r#type: Some(game_message::Type::State(game_message::StateMsg {
                state: state.clone(),
            })),
        };

        // Отправляем каждому известному игроку
        for (player_id, addr) in known_players {
            if player_id != 1 { // Не отправляем себе
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
    last_state: &Arc<Mutex<Option<GameState>>>,
    state_manager: &Arc<Mutex<Box<dyn StateManager>>>,
    players: &Arc<Mutex<GamePlayers>>,
    game_config: &Arc<Mutex<Option<GameConfig>>>,
    net: &Arc<UdpNetwork>,
    msg: GameMessage,
    addr: SocketAddr,
) {
    let now = Instant::now();

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
                        
                        DiscoveredGameDto {
                            game_name: game.announcement.game_name.clone(),
                            players_count: game.announcement.players.players.len(),
                            can_join: game.announcement.can_join.unwrap_or(true),
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
            return;
        }
        _ => {}
    }

    // Обрабатываем State отдельно (для обновления UI)
    if let Some(game_message::Type::State(state_msg)) = msg.r#type.as_ref() {
        let mut state_guard = last_state.lock().expect("state mutex poisoned");
        *state_guard = Some(state_msg.state.clone());
        
        // Обновляем players из state
        let mut players_guard = players.lock().expect("players mutex poisoned");
        *players_guard = state_msg.state.players.clone();
        drop(players_guard);
        drop(state_guard);

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

    // Все остальные сообщения обрабатываем через StateManager
    let mut state_mgr = state_manager.lock().expect("state_manager mutex poisoned");
    let mut players_guard = players.lock().expect("players mutex poisoned");
    
    if let Err(e) = state_mgr.handle_message(msg.clone(), addr, &**net, &mut players_guard) {
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
                    #[derive(Clone, serde::Serialize)]
                    struct JoinEvent {
                        player_name: String,
                        player_id: i32,
                    }
                    let _ = app.emit("player-joined", JoinEvent {
                        player_name: player.name.clone(),
                        player_id: player.id,
                    });
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
