pub mod protocol;
pub mod udp_impl;

use tauri::Emitter;
use crate::snakes::{
    game_message, game_message::RoleChangeMsg, GameAnnouncement, GameMessage, GamePlayer,
    GameState, NodeRole, PlayerType,
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
pub struct GameStateDto {
    pub state_order: i32,
    pub snakes: Vec<SnakeDto>,
    pub foods: Vec<CoordDto>,
    pub players: GamePlayersDto,
}

#[derive(Clone)]
pub struct NetworkService {
    net: Arc<UdpNetwork>,
    discovered: Arc<Mutex<Vec<DiscoveredGame>>>,
    current_master: Arc<Mutex<Option<SocketAddr>>>,
    last_state: Arc<Mutex<Option<GameState>>>,
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
        let running = Arc::clone(&self.running);
        let app_handle = app.clone();

        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                match net.poll_receive() {
                    Ok(Some((msg, addr))) => {
                        println!("Received message from {}: {:?}", addr, msg);
                        process_message(&app_handle, &discovered, &last_state, msg, addr);
                    }
                    Ok(None) => {
                        // std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(err) => {
                        eprintln!("Network receive error: {err:#}");
                        // std::thread::sleep(Duration::from_millis(50));
                    }
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
            .map(|game| DiscoveredGameDto {
                game_name: game.announcement.game_name.clone(),
                players_count: game.announcement.players.players.len(),
                can_join: game.announcement.can_join.unwrap_or(true),
                master_address: game.master_address.to_string(),
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
        state.as_ref().map(game_state_to_dto)
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
    msg: GameMessage,
    addr: SocketAddr,
) {
    let now = Instant::now();

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
                    .map(|game| DiscoveredGameDto {
                        game_name: game.announcement.game_name.clone(),
                        players_count: game.announcement.players.players.len(),
                        can_join: game.announcement.can_join.unwrap_or(true),
                        master_address: game.master_address.to_string(),
                    })
                    .collect(),
            };
            let _ = app.emit("games-discovered", payload);
        }
        Some(game_message::Type::State(state_msg)) => {
            let mut state_guard = last_state.lock().expect("state mutex poisoned");
            *state_guard = Some(state_msg.state.clone());
            drop(state_guard);

            let payload = game_state_to_dto(&state_msg.state);
            let _ = app.emit("game-state", payload);
        }
        Some(game_message::Type::Error(error_msg)) => {
            let _ = app.emit("game-error", error_msg.error_message.clone());
        }
        Some(game_message::Type::Discover(_)) => {
            let _ = app.emit("network-event", "discover");
        }
        Some(game_message::Type::Join(_)) => {
            let _ = app.emit("network-event", "join");
        }
        Some(game_message::Type::Ping(_)) => {
            let _ = app.emit("network-event", "ping");
        }
        Some(game_message::Type::Ack(_)) => {
            let _ = app.emit("network-event", "ack");
        }
        Some(game_message::Type::Steer(_)) => {
            let _ = app.emit("network-event", "steer");
        }
        Some(game_message::Type::RoleChange(_)) => {
            let _ = app.emit("network-event", "role-change");
        }
        _ => {
            let _ = app.emit("network-event", "unknown");
        }
    }
}

fn game_state_to_dto(state: &GameState) -> GameStateDto {
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
