use crate::network::protocol::NetworkProtocol;
use crate::snakes::{
    GameMessage, GamePlayer, GamePlayers, NodeRole,
    game_message::{self, AckMsg, PingMsg, RoleChangeMsg},
};
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tauri::Emitter;

#[derive(Clone, PartialEq, Debug)]
pub enum GameMode {
    Lobby,
    InGame {
        role: NodeRole,
        master_addr: Option<SocketAddr>,
        deputy_id: Option<i32>,
    },
    Viewer,
}

pub trait StateManager: Send + Sync {
    fn current_mode(&self) -> GameMode;
    fn my_id(&self) -> i32;
    fn transition(&mut self, new_mode: GameMode, net: &dyn NetworkProtocol) -> Result<()>;
    fn handle_message(
        &mut self,
        msg: GameMessage,
        sender: SocketAddr,
        net: &dyn NetworkProtocol,
        players: &mut GamePlayers,
        app: &tauri::AppHandle,
    ) -> Result<()>;
    fn pick_deputy(&self, players: &GamePlayers) -> Option<i32>;
    fn check_timeouts(
        &mut self,
        delay_ms: u32,
        net: &dyn NetworkProtocol,
        players: &mut GamePlayers,
    ) -> Result<()>;
    fn send_ping_if_needed(
        &mut self,
        delay_ms: u32,
        net: &dyn NetworkProtocol,
    ) -> Result<()>;
    fn get_known_players(&self) -> Vec<(i32, SocketAddr)>;
}

pub struct StateImpl {
    mode: GameMode,
    my_id: i32,
    known_players: HashMap<i32, (GamePlayer, SocketAddr, Instant)>,
    last_send_times: HashMap<SocketAddr, Instant>,
    seq_counter: i64,
}

impl StateImpl {
    pub fn new() -> Self {
        Self {
            mode: GameMode::Lobby,
            my_id: 0,
            known_players: HashMap::new(),
            last_send_times: HashMap::new(),
            seq_counter: 0,
        }
    }
}

impl StateManager for StateImpl {
    fn current_mode(&self) -> GameMode {
        self.mode.clone()
    }

    fn my_id(&self) -> i32 {
        self.my_id
    }

    fn transition(&mut self, new_mode: GameMode, _net: &dyn NetworkProtocol) -> Result<()> {
        match (&self.mode, &new_mode) {
            (
                GameMode::Lobby,
                GameMode::InGame {
                    role: NodeRole::Master,
                    ..
                },
            ) => {
                self.my_id = 1;
                self.mode = new_mode;
                Ok(())
            }

            (GameMode::Lobby, GameMode::InGame { .. }) => {
                self.my_id = 0;
                self.mode = new_mode;
                Ok(())
            }

            (_, GameMode::Lobby) => {
                self.mode = GameMode::Lobby;
                self.my_id = 0;
                self.known_players.clear();
                self.last_send_times.clear();
                self.seq_counter = 0;
                Ok(())
            }

            _ => Ok(()),
        }
    }

    fn handle_message(
        &mut self,
        msg: GameMessage,
        sender: SocketAddr,
        net: &dyn NetworkProtocol,
        players: &mut GamePlayers,
        app: &tauri::AppHandle,
    ) -> Result<()> {
        let mut ack_already_sent = false;

        if matches!(msg.r#type.as_ref(), Some(game_message::Type::Ack(_))) {
            if self.my_id == 0 {
                if let Some(id) = msg.receiver_id {
                    self.my_id = id;
                }
            }
        }

        if let Some(sender_id) = msg.sender_id {
            let entry = self.known_players.entry(sender_id).or_insert_with(|| {
                (
                    GamePlayer {
                        name: String::new(),
                        id: sender_id,
                        ip_address: None,
                        port: None,
                        role: NodeRole::Normal as i32,
                        r#type: None,
                        score: 0,
                    },
                    sender,
                    Instant::now(),
                )
            });
            entry.1 = sender;
            entry.2 = Instant::now();
        }

        match (&self.mode, msg.r#type.as_ref()) {
            (
                GameMode::InGame {
                    role: NodeRole::Master,
                    ..
                },
                Some(game_message::Type::Join(join)),
            ) => {
                let new_id = self.select_player_id(players);
                players.players.push(GamePlayer {
                    name: join.player_name.clone(),
                    id: new_id,
                    ip_address: None,
                    port: None,
                    role: join.requested_role,
                    r#type: join.player_type,
                    score: 0,
                });

                self.known_players
                    .entry(new_id)
                    .and_modify(|e| {
                        e.0.id = new_id;
                        e.0.name = join.player_name.clone();
                        e.0.role = join.requested_role;
                        e.0.r#type = join.player_type;
                        e.1 = sender;
                        e.2 = Instant::now();
                    })
                    .or_insert_with(|| {
                        (
                            GamePlayer {
                                name: join.player_name.clone(),
                                id: new_id,
                                ip_address: None,
                                port: None,
                                role: join.requested_role,
                                r#type: join.player_type,
                                score: 0,
                            },
                            sender,
                            Instant::now(),
                        )
                    });

                let ack = GameMessage {
                    msg_seq: msg.msg_seq,
                    sender_id: Some(self.my_id),
                    receiver_id: Some(new_id),
                    r#type: Some(game_message::Type::Ack(AckMsg {})),
                };
                net.send_unicast(sender, ack)?;
                self.last_send_times.insert(sender, Instant::now());
                ack_already_sent = true;
            }

            (
                GameMode::InGame {
                    role: NodeRole::Master,
                    master_addr,
                    deputy_id,
                },
                Some(game_message::Type::RoleChange(rc)),
            ) => {
                // RoleChange от игрока со sender_role=VIEWER:
                // - receiver_role=None  => стать зрителем (остаться подключенным)
                // - receiver_role=VIEWER => выйти из игры полностью (отписываемся, не шлем State)
                if rc.sender_role == Some(NodeRole::Viewer as i32) {
                    if let Some(sender_id) = msg.sender_id {
                        let disconnect = rc.receiver_role == Some(NodeRole::Viewer as i32);
                        if disconnect {
                            println!("Player {} is leaving the game (disconnect)", sender_id);
                        } else {
                            println!("Player {} is becoming a viewer", sender_id);
                        }

                        if let Some(p) = players.players.iter_mut().find(|p| p.id == sender_id) {
                            p.role = NodeRole::Viewer as i32;
                        }

                        // В обоих случаях змейка становится ZOMBIE
                        #[derive(Clone, serde::Serialize)]
                        struct ZombieEvent {
                            player_id: i32,
                        }
                        let _ = app.emit(
                            "player-became-zombie",
                            ZombieEvent {
                                player_id: sender_id,
                            },
                        );

                        // Только при disconnect удаляем из known_players (перестаем слать State)
                        if disconnect {
                            self.known_players.remove(&sender_id);
                        }

                        // Если это был deputy — выбираем нового deputy
                        if *deputy_id == Some(sender_id) {
                            let new_dep = self.pick_deputy(players);
                            self.mode = GameMode::InGame {
                                role: NodeRole::Master,
                                master_addr: *master_addr,
                                deputy_id: new_dep,
                            };

                            if let Some(new_dep) = new_dep {
                                if let Some(addr) = self.known_players.get(&new_dep).map(|e| e.1) {
                                    let role_msg = GameMessage {
                                        msg_seq: self.next_seq(),
                                        sender_id: Some(self.my_id),
                                        receiver_id: Some(new_dep),
                                        r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                                            sender_role: Some(NodeRole::Master as i32),
                                            receiver_role: Some(NodeRole::Deputy as i32),
                                        })),
                                    };
                                    let _ = net.send_unicast(addr, role_msg);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        let should_ack = match msg.r#type {
            Some(game_message::Type::Announcement(_)) => false,
            Some(game_message::Type::Discover(_)) => false,
            Some(game_message::Type::Ack(_)) => false,
            None => false,
            _ => true,
        };

        if should_ack && !ack_already_sent {
            let ack = GameMessage {
                msg_seq: msg.msg_seq,
                sender_id: Some(self.my_id),
                receiver_id: msg.sender_id,
                r#type: Some(game_message::Type::Ack(AckMsg {})),
            };
            net.send_unicast(sender, ack)?;
            self.last_send_times.insert(sender, Instant::now());
        }

        Ok(())
    }

    fn pick_deputy(&self, players: &GamePlayers) -> Option<i32> {
        let candidates: Vec<i32> = players
            .players
            .iter()
            .filter(|p| p.role == NodeRole::Normal as i32)
            .map(|p| p.id)
            .collect();

        if candidates.is_empty() {
            None
        } else {
            Some(candidates[0])
        }
    }

    fn check_timeouts(
        &mut self,
        delay_ms: u32,
        net: &dyn NetworkProtocol,
        players: &mut GamePlayers,
    ) -> Result<()> {
        let timeout = Duration::from_millis((delay_ms as f32 * 0.8) as u64);
        let mut dropouts = Vec::new();
        let now = Instant::now();

        for (id, (_, _, last_seen)) in &self.known_players {
            if now.duration_since(*last_seen) > timeout {
                dropouts.push(*id);
            }
        }

        for id in dropouts {
            let dropped = self.known_players.remove(&id);
            let dropped_addr = dropped.as_ref().map(|e| e.1);
            let dropped_role = dropped.as_ref().map(|e| e.0.role);

            if let Some(p) = players.players.iter_mut().find(|p| p.id == id) {
                p.role = NodeRole::Viewer as i32;
            }

            if let GameMode::InGame {
                role,
                master_addr,
                deputy_id,
            } = self.mode.clone()
            {
                match role {
                    NodeRole::Master => {
                        if deputy_id == Some(id) {
                            let new_dep = self.pick_deputy(players);

                            self.mode = GameMode::InGame {
                                role,
                                master_addr,
                                deputy_id: new_dep,
                            };

                            if let Some(new_dep) = new_dep {
                                let new_addr = self.known_players.get(&new_dep).map(|e| e.1);
                                if let Some(addr) = new_addr {
                                    let msg = GameMessage {
                                        msg_seq: self.next_seq(),
                                        sender_id: Some(self.my_id),
                                        receiver_id: Some(new_dep),
                                        r#type: Some(game_message::Type::RoleChange(
                                            RoleChangeMsg {
                                                sender_role: Some(NodeRole::Master as i32),
                                                receiver_role: Some(NodeRole::Deputy as i32),
                                            },
                                        )),
                                    };
                                    net.send_unicast(addr, msg)?;
                                }
                            }
                        }
                    }
                    NodeRole::Deputy => {
                        let master_disappeared = master_addr.is_some()
                            && (dropped_role == Some(NodeRole::Master as i32)
                                || (dropped_addr.is_some() && dropped_addr == master_addr));

                        if master_disappeared {
                            let new_dep = self.pick_deputy(players);
                            self.mode = GameMode::InGame {
                                role: NodeRole::Master,
                                master_addr: None,
                                deputy_id: new_dep,
                            };
                            net.set_role(NodeRole::Master);

                            let targets: Vec<(i32, SocketAddr)> = self
                                .known_players
                                .iter()
                                .map(|(id, (_p, addr, _))| (*id, *addr))
                                .collect();

                            for (target_id, addr) in targets {
                                if self.my_id != 0 && target_id == self.my_id {
                                    continue;
                                }

                                let receiver_role = if new_dep == Some(target_id) {
                                    Some(NodeRole::Deputy as i32)
                                } else {
                                    None
                                };

                                let msg = GameMessage {
                                    msg_seq: self.next_seq(),
                                    sender_id: Some(self.my_id),
                                    receiver_id: Some(target_id),
                                    r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                                        sender_role: Some(NodeRole::Master as i32),
                                        receiver_role,
                                    })),
                                };
                                net.send_unicast(addr, msg)?;
                                self.last_send_times.insert(addr, now);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn send_ping_if_needed(&mut self, delay_ms: u32, net: &dyn NetworkProtocol) -> Result<()> {
        let interval = Duration::from_millis(((delay_ms as f32 * 0.5).max(500.0)) as u64);
        let now = Instant::now();

        for (_, (_, addr, _)) in &self.known_players {
            self.last_send_times.entry(*addr).or_insert(now);
        }

        let addrs: Vec<SocketAddr> = self.last_send_times.keys().copied().collect();
        for addr in addrs {
            let last = *self.last_send_times.get(&addr).unwrap_or(&now);
            if now.duration_since(last) > interval {
                let ping = GameMessage {
                    msg_seq: self.next_seq(),
                    sender_id: Some(self.my_id),
                    receiver_id: None,
                    r#type: Some(game_message::Type::Ping(PingMsg {})),
                };
                net.send_unicast(addr, ping)?;
                self.last_send_times.insert(addr, now);
            }
        }

        Ok(())
    }

    fn get_known_players(&self) -> Vec<(i32, SocketAddr)> {
        self.known_players
            .iter()
            .map(|(id, (_player, addr, _last_seen))| (*id, *addr))
            .collect()
    }
}

impl StateImpl {
    fn next_seq(&mut self) -> i64 {
        self.seq_counter += 1;
        self.seq_counter
    }

    fn select_player_id(&self, players: &GamePlayers) -> i32 {
        players.players.iter().map(|p| p.id).max().unwrap_or(0) + 1
    }
}
