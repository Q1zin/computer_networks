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

    /// Подмешать в known_players адреса игроков из актуального списка players,
    /// если в нём есть ip/port. Нужно, чтобы deputy знал адреса NORMAL'ов.
    fn observe_players(&mut self, players: &GamePlayers);

    /// Для случая, когда MASTER добровольно становится зрителем: переключаемся на VIEWER,
    /// но остаёмся в InGame и продолжаем принимать state от нового мастера.
    fn become_viewer(
        &mut self,
        master_addr: Option<SocketAddr>,
        deputy_id: Option<i32>,
        net: &dyn NetworkProtocol,
    ) -> Result<()>;
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
        app: &tauri::AppHandle,
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
    removed_players: std::collections::HashSet<i32>, // Игроки, которые вышли из игры
    last_send_times: HashMap<SocketAddr, Instant>,
    seq_counter: i64,
}

impl StateImpl {
    pub fn new() -> Self {
        Self {
            mode: GameMode::Lobby,
            my_id: 0,
            known_players: HashMap::new(),
            removed_players: std::collections::HashSet::new(),
            last_send_times: HashMap::new(),
            seq_counter: 0,
        }
    }

    fn ensure_deputy_for_master(
        &mut self,
        net: &dyn NetworkProtocol,
        players: &mut GamePlayers,
    ) -> Result<()> {
        let (master_addr, deputy_id) = match self.mode.clone() {
            GameMode::InGame {
                role: NodeRole::Master,
                master_addr,
                deputy_id,
            } => (master_addr, deputy_id),
            _ => return Ok(()),
        };

        let deputy_is_alive = deputy_id
            .and_then(|id| {
                let is_known = self.known_players.contains_key(&id);
                let is_deputy_role = players
                    .players
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.role == NodeRole::Deputy as i32)
                    .unwrap_or(false);
                if is_known && is_deputy_role {
                    Some(id)
                } else {
                    None
                }
            })
            .is_some();

        if deputy_is_alive {
            return Ok(());
        }

        // Убираем старые отметки DEPUTY (если остались) и выбираем нового среди NORMAL.
        for p in players.players.iter_mut() {
            if p.role == NodeRole::Deputy as i32 {
                p.role = NodeRole::Normal as i32;
            }
        }

        let new_dep = self.pick_deputy(players);
        self.mode = GameMode::InGame {
            role: NodeRole::Master,
            master_addr,
            deputy_id: new_dep,
        };

        if let Some(new_dep) = new_dep {
            if let Some(p) = players.players.iter_mut().find(|p| p.id == new_dep) {
                p.role = NodeRole::Deputy as i32;
            }

            if let Some(addr) = self.known_players.get(&new_dep).map(|e| e.1) {
                let msg = GameMessage {
                    msg_seq: self.next_seq(),
                    sender_id: Some(self.my_id),
                    receiver_id: Some(new_dep),
                    r#type: Some(game_message::Type::RoleChange(RoleChangeMsg {
                        sender_role: Some(NodeRole::Master as i32),
                        receiver_role: Some(NodeRole::Deputy as i32),
                    })),
                };
                net.send_unicast(addr, msg)?;
                self.last_send_times.insert(addr, Instant::now());
            }
        }

        Ok(())
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
                self.removed_players.clear();
                self.last_send_times.clear();
                self.seq_counter = 0;
                Ok(())
            }

            _ => Ok(()),
        }
    }

    fn observe_players(&mut self, players: &GamePlayers) {
        let now = Instant::now();
        for p in &players.players {
            // Не добавляем игроков, которые уже вышли из игры
            if self.removed_players.contains(&p.id) {
                continue;
            }
            
            let Some(ip) = p.ip_address.as_ref() else { continue };
            let Some(port) = p.port else { continue };
            let Ok(addr) = format!("{}:{}", ip, port).parse::<SocketAddr>() else { continue };

            self.known_players
                .entry(p.id)
                .and_modify(|e| {
                    e.0 = p.clone();
                    e.1 = addr;
                    // last_seen тут не обязательно обновлять, но это полезно для
                    // поддержания живости адресов при работе через state.
                    e.2 = now;
                })
                .or_insert_with(|| (p.clone(), addr, now));
        }
    }

    fn become_viewer(
        &mut self,
        master_addr: Option<SocketAddr>,
        deputy_id: Option<i32>,
        net: &dyn NetworkProtocol,
    ) -> Result<()> {
        match self.mode.clone() {
            GameMode::InGame { .. } => {
                self.mode = GameMode::InGame {
                    role: NodeRole::Viewer,
                    master_addr,
                    deputy_id,
                };
                net.set_role(NodeRole::Viewer);
            }
            _ => {
                self.mode = GameMode::Viewer;
                net.set_role(NodeRole::Viewer);
            }
        }
        Ok(())
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

            // MASTER должен уметь распространять адреса игроков через State,
            // иначе deputy при takeover не знает куда слать State/RoleChange.
            if let Some(p) = players.players.iter_mut().find(|p| p.id == sender_id) {
                p.ip_address = Some(sender.ip().to_string());
                p.port = Some(sender.port() as i32);
            }
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
                    ip_address: Some(sender.ip().to_string()),
                    port: Some(sender.port() as i32),
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

                // После изменения состава игроков — гарантируем, что есть живой DEPUTY (если возможно).
                self.ensure_deputy_for_master(net, players)?;
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
                            self.removed_players.insert(sender_id); // Запоминаем, что игрок вышел
                            
                            // Удаляем игрока из списка на фронтенде
                            #[derive(Clone, serde::Serialize)]
                            struct PlayerLeftEvent {
                                player_id: i32,
                            }
                            let _ = app.emit("player-left", PlayerLeftEvent { player_id: sender_id });
                            
                            // Удаляем из players
                            players.players.retain(|p| p.id != sender_id);
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

                        // После изменения состава/ролей игроков — гарантируем deputy.
                        self.ensure_deputy_for_master(net, players)?;
                    }
                }
            }

            (
                GameMode::InGame {
                    role,
                    master_addr: _,
                    deputy_id,
                },
                Some(game_message::Type::RoleChange(rc)),
            ) => {
                // RoleChange от мастера используется для:
                // - назначения DEPUTY (receiver_role=DEPUTY)
                // - оповещения о смене мастера (sender_role=MASTER)
                if rc.sender_role == Some(NodeRole::Master as i32) {
                    let new_master_addr = sender;

                    // RoleChange может прийти раньше Ack (UDP), но он unicast на наш сокет,
                    // поэтому receiver_id можно использовать, чтобы установить my_id.
                    if self.my_id == 0 {
                        if let Some(id) = msg.receiver_id {
                            self.my_id = id;
                        }
                    }

                    // Если мастер назначил deputy — фиксируем это локально
                    if rc.receiver_role == Some(NodeRole::Deputy as i32) {
                        if let Some(target_id) = msg.receiver_id {
                            if let Some(p) = players.players.iter_mut().find(|p| p.id == target_id)
                            {
                                p.role = NodeRole::Deputy as i32;
                            }

                            // Если это мы — становимся deputy
                            if self.my_id != 0 && target_id == self.my_id {
                                self.mode = GameMode::InGame {
                                    role: NodeRole::Deputy,
                                    master_addr: Some(new_master_addr),
                                    deputy_id: Some(target_id),
                                };
                                net.set_role(NodeRole::Deputy);
                                return Ok(());
                            }

                            // Иначе — запоминаем deputy_id и остаёмся в своей роли
                            if let GameMode::InGame {
                                role: my_role,
                                master_addr: _,
                                deputy_id: _,
                            } = self.mode.clone()
                            {
                                // Если раньше мы были deputy, но назначили другого — откатываемся в normal
                                let effective_role = if my_role == NodeRole::Deputy {
                                    NodeRole::Normal
                                } else {
                                    my_role
                                };

                                self.mode = GameMode::InGame {
                                    role: effective_role,
                                    master_addr: Some(new_master_addr),
                                    deputy_id: Some(target_id),
                                };
                                if effective_role == NodeRole::Normal {
                                    net.set_role(NodeRole::Normal);
                                }
                                return Ok(());
                            }
                        }
                    }

                    // Явное сообщение от старого мастера: "ты теперь MASTER".
                    // Используем receiver_role=MASTER и receiver_id=target.
                    if rc.receiver_role == Some(NodeRole::Master as i32) {
                        if let Some(target_id) = msg.receiver_id {
                            if self.my_id != 0 && target_id == self.my_id {
                                // Если мы уже MASTER, повторный handoff игнорируем.
                                if matches!(self.mode, GameMode::InGame { role: NodeRole::Master, .. }) {
                                    return Ok(());
                                }

                                // Становимся мастером и запускаем симуляцию (через событие).
                                #[derive(Clone, serde::Serialize)]
                                struct BecameMasterEvent {
                                    old_master_id: i32,
                                }

                                let old_master_id = msg.sender_id.unwrap_or(0);
                                let _ = app.emit(
                                    "became-master",
                                    BecameMasterEvent { old_master_id },
                                );

                                // Обновляем роли в players: старый мастер -> VIEWER, мы -> MASTER,
                                // выбираем нового deputy (если возможно).
                                let old_master_id = msg.sender_id.unwrap_or(0);
                                for p in players.players.iter_mut() {
                                    if p.id == old_master_id {
                                        p.role = NodeRole::Viewer as i32;
                                    }
                                    if p.id == self.my_id {
                                        p.role = NodeRole::Master as i32;
                                    }
                                    if p.role == NodeRole::Deputy as i32 {
                                        p.role = NodeRole::Normal as i32;
                                    }
                                }

                                // Добавляем старого master в known_players как Viewer,
                                // чтобы он продолжал получать State.
                                if old_master_id != 0 {
                                    let old_master_player = players
                                        .players
                                        .iter()
                                        .find(|p| p.id == old_master_id)
                                        .cloned()
                                        .unwrap_or_else(|| GamePlayer {
                                            name: String::new(),
                                            id: old_master_id,
                                            ip_address: None,
                                            port: None,
                                            role: NodeRole::Viewer as i32,
                                            r#type: None,
                                            score: 0,
                                        });
                                    println!("[RoleChange->Master] Adding old_master_id={} at addr={} to known_players", old_master_id, sender);
                                    self.known_players
                                        .entry(old_master_id)
                                        .and_modify(|e| {
                                            e.0.role = NodeRole::Viewer as i32;
                                            e.1 = sender; // адрес отправителя RoleChange
                                            e.2 = Instant::now();
                                        })
                                        .or_insert_with(|| (old_master_player, sender, Instant::now()));
                                    println!("[RoleChange->Master] known_players now has {} entries: {:?}", 
                                        self.known_players.len(),
                                        self.known_players.keys().collect::<Vec<_>>());
                                }

                                let new_dep = self.pick_deputy(players);
                                if let Some(dep_id) = new_dep {
                                    if let Some(p) = players.players.iter_mut().find(|p| p.id == dep_id) {
                                        p.role = NodeRole::Deputy as i32;
                                    }
                                }

                                self.mode = GameMode::InGame {
                                    role: NodeRole::Master,
                                    master_addr: None,
                                    deputy_id: new_dep,
                                };
                                net.set_role(NodeRole::Master);
                                return Ok(());
                            }
                        }
                    }

                    // Сообщение "я теперь мастер" (deputy promotion) — обновляем адрес мастера.
                    if let GameMode::InGame {
                        role: my_role,
                        master_addr: _,
                        deputy_id: cur_dep,
                    } = self.mode.clone()
                    {
                        let effective_role = match my_role {
                            NodeRole::Master => NodeRole::Master,
                            NodeRole::Deputy => NodeRole::Normal,
                            _ => my_role,
                        };
                        if effective_role != my_role {
                            net.set_role(effective_role);
                        }

                        self.mode = GameMode::InGame {
                            role: effective_role,
                            master_addr: Some(new_master_addr),
                            deputy_id: cur_dep,
                        };
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

        // VIEWER должен отправлять ACK на State, иначе новый master удалит его
        // из known_players по таймауту и перестанет слать State.
        // Однако VIEWER не шлёт ничего кроме ACK (не Steer, не Ping, не RoleChange).

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
        app: &tauri::AppHandle,
    ) -> Result<()> {
        let timeout = Duration::from_millis(((delay_ms as u64) * 8) / 10);
        let mut dropouts = Vec::new();
        let now = Instant::now();

        for (id, (_, _, last_seen)) in &self.known_players {
            // Себя никогда не таймаутим: иначе при редких входящих пакетах можно
            // случайно превратиться в VIEWER и начать слать себе ping/ack.
            if self.my_id != 0 && *id == self.my_id {
                continue;
            }
            if now.duration_since(*last_seen) > timeout {
                dropouts.push(*id);
            }
        }

        for id in dropouts {
            let dropped = self.known_players.remove(&id);
            self.removed_players.insert(id); // Запоминаем, что игрок вышел
            let dropped_addr = dropped.as_ref().map(|e| e.1);
            let dropped_role = dropped.as_ref().map(|e| e.0.role);

            let mut was_viewer = false;

            if let Some(p) = players.players.iter_mut().find(|p| p.id == id) {
                was_viewer = p.role == NodeRole::Viewer as i32;
                p.role = NodeRole::Viewer as i32;
            }
            
            // Удаляем игрока из списка на фронтенде при timeout
            #[derive(Clone, serde::Serialize)]
            struct PlayerLeftEvent {
                player_id: i32,
            }
            let _ = app.emit("player-left", PlayerLeftEvent { player_id: id });
            
            // Удаляем из players
            players.players.retain(|p| p.id != id);

            if let GameMode::InGame {
                role,
                master_addr,
                deputy_id,
            } = self.mode.clone()
            {
                match role {
                    NodeRole::Master => {
                        // На MASTER любой отвалившийся игрок превращается в ZOMBIE,
                        // но если он уже VIEWER (например, после явного handoff),
                        // повторно не эмитим событие, чтобы не плодить дубликаты.
                        if !was_viewer {
                            #[derive(Clone, serde::Serialize)]
                            struct ZombieEvent {
                                player_id: i32,
                            }
                            let _ = app.emit(
                                "player-became-zombie",
                                ZombieEvent { player_id: id },
                            );
                        }

                        if deputy_id == Some(id) {
                            let new_dep = self.pick_deputy(players);

                            // Обновляем роль нового deputy в списке игроков (для UI/announcement)
                            if let Some(new_dep) = new_dep {
                                if let Some(p) = players.players.iter_mut().find(|p| p.id == new_dep)
                                {
                                    p.role = NodeRole::Deputy as i32;
                                }
                            }

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

                        // Даже если отвалился не deputy: если deputy отсутствует — назначаем нового.
                        self.ensure_deputy_for_master(net, players)?;
                    }
                    NodeRole::Deputy => {
                        let master_disappeared = master_addr.is_some()
                            && (dropped_role == Some(NodeRole::Master as i32)
                                || (dropped_addr.is_some() && dropped_addr == master_addr));

                        if master_disappeared {
                            #[derive(Clone, serde::Serialize)]
                            struct BecameMasterEvent {
                                old_master_id: i32,
                            }

                            // MASTER "умирает" => на новом мастере его змейка должна стать ZOMBIE,
                            // а роль игрока уже переведена в VIEWER выше.
                            #[derive(Clone, serde::Serialize)]
                            struct ZombieEvent {
                                player_id: i32,
                            }
                            let _ = app.emit(
                                "player-became-zombie",
                                ZombieEvent { player_id: id },
                            );

                            // Сообщаем приложению, что мы стали MASTER и должны поднять симуляцию.
                            // В payload передаём id старого мастера, чтобы гарантированно превратить
                            // его змейку в ZOMBIE даже если слушатели ещё не были зарегистрированы.
                            let _ = app.emit(
                                "became-master",
                                BecameMasterEvent { old_master_id: id },
                            );

                            let new_dep = self.pick_deputy(players);
                            self.mode = GameMode::InGame {
                                role: NodeRole::Master,
                                master_addr: None,
                                deputy_id: new_dep,
                            };
                            net.set_role(NodeRole::Master);

                            // Обновляем роли в players: мы теперь MASTER, новый deputy (если есть).
                            for p in &mut players.players {
                                if p.id == self.my_id {
                                    p.role = NodeRole::Master as i32;
                                } else if Some(p.id) == new_dep {
                                    p.role = NodeRole::Deputy as i32;
                                } else if p.role == NodeRole::Deputy as i32 {
                                    // Сбрасываем старую роль deputy (если была у кого-то другого)
                                    p.role = NodeRole::Normal as i32;
                                }
                            }

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
                    NodeRole::Normal => {
                        // (a) NORMAL заметил, что отвалился MASTER => переключаемся на DEPUTY
                        let master_disappeared = master_addr.is_some()
                            && (dropped_role == Some(NodeRole::Master as i32)
                                || (dropped_addr.is_some() && dropped_addr == master_addr));

                        if master_disappeared {
                            let dep_id = deputy_id.or_else(|| {
                                players
                                    .players
                                    .iter()
                                    .find(|p| p.role == NodeRole::Deputy as i32)
                                    .map(|p| p.id)
                            });

                            if let Some(dep_id) = dep_id {
                                if let Some(dep_addr) = self.known_players.get(&dep_id).map(|e| e.1)
                                {
                                    self.mode = GameMode::InGame {
                                        role: NodeRole::Normal,
                                        master_addr: Some(dep_addr),
                                        deputy_id: Some(dep_id),
                                    };
                                    self.last_send_times.insert(dep_addr, now);
                                }
                            } else {
                                // Нет deputy - игра завершается, выходим в lobby
                                println!("[check_timeouts] Master gone, no deputy - game over, returning to lobby");
                                let _ = app.emit("game-over", "No master or deputy available");
                                self.mode = GameMode::Lobby;
                                self.known_players.clear();
                                self.removed_players.clear();
                            }
                        }
                    }
                    NodeRole::Viewer => {
                        // Viewer тоже должен выйти в lobby если master пропал и deputy нет
                        let master_disappeared = master_addr.is_some()
                            && (dropped_role == Some(NodeRole::Master as i32)
                                || (dropped_addr.is_some() && dropped_addr == master_addr));

                        if master_disappeared {
                            let dep_id = deputy_id.or_else(|| {
                                players
                                    .players
                                    .iter()
                                    .find(|p| p.role == NodeRole::Deputy as i32)
                                    .map(|p| p.id)
                            });

                            if let Some(dep_id) = dep_id {
                                if let Some(dep_addr) = self.known_players.get(&dep_id).map(|e| e.1)
                                {
                                    self.mode = GameMode::InGame {
                                        role: NodeRole::Viewer,
                                        master_addr: Some(dep_addr),
                                        deputy_id: Some(dep_id),
                                    };
                                    self.last_send_times.insert(dep_addr, now);
                                }
                            } else {
                                // Нет deputy - игра завершается, выходим в lobby
                                println!("[check_timeouts] Master gone, no deputy - game over, returning to lobby");
                                let _ = app.emit("game-over", "No master or deputy available");
                                self.mode = GameMode::Lobby;
                                self.known_players.clear();
                                self.removed_players.clear();
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
        // По протоколу: если не отправляли никаких unicast-сообщений узлу
        // в течение state_delay_ms / 10, необходимо отправить PingMsg.
        // Это касается ВСЕХ узлов (Master, Deputy, Normal, Viewer).

        // Если наш id ещё не установлен, безопаснее быть полностью молчаливым,
        // чем случайно пинговать самого себя.
        if self.my_id == 0 {
            return Ok(());
        }

        // Viewer и Normal шлют ping только мастеру, Master — всем known_players.
        let is_master = matches!(
            self.mode,
            GameMode::InGame {
                role: NodeRole::Master,
                ..
            }
        );

        let delay_ms = delay_ms.max(1) as u64;
        let interval = Duration::from_millis(delay_ms / 10);
        let now = Instant::now();

        let my_addr = self.known_players.get(&self.my_id).map(|e| e.1);

        // Определяем кому нужно слать ping:
        // - Master шлёт всем в known_players
        // - Остальные (Deputy, Normal, Viewer) шлют только мастеру
        let targets: Vec<SocketAddr> = if is_master {
            // Master: всем known_players
            for (_, (_, addr, _)) in &self.known_players {
                self.last_send_times.entry(*addr).or_insert(now);
            }
            self.last_send_times.keys().copied().collect()
        } else {
            // Не-master: только мастеру
            match &self.mode {
                GameMode::InGame { master_addr: Some(addr), .. } => {
                    self.last_send_times.entry(*addr).or_insert(now);
                    vec![*addr]
                }
                GameMode::Viewer => {
                    // Viewer тоже должен знать master_addr, но его нет в этом режиме.
                    // Попробуем найти мастера в known_players.
                    let master_addr = self.known_players
                        .values()
                        .find(|(p, _, _)| p.role == NodeRole::Master as i32)
                        .map(|(_, addr, _)| *addr);
                    if let Some(addr) = master_addr {
                        self.last_send_times.entry(addr).or_insert(now);
                        vec![addr]
                    } else {
                        vec![]
                    }
                }
                _ => vec![],
            }
        };

        for addr in targets {
            if my_addr == Some(addr) {
                continue;
            }
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
