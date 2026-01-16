use crate::snakes::{
    Direction, GameConfig, GamePlayer, GamePlayers, GameState, NodeRole,
    game_state::snake::SnakeState,
    game_state::{Coord, Snake},
};
use rand::SeedableRng;
use anyhow::{Result, anyhow};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::{HashMap, HashSet};

pub trait GameField: Send + Sync {
    fn new(config: GameConfig, players: GamePlayers) -> Self
    where
        Self: Sized;
    fn update(&mut self, steers: HashMap<i32, Direction>) -> Result<GameState>;
    fn place_new_snake(&mut self, player_name: String) -> Result<i32>;
    fn get_current_state(&self) -> GameState;
    fn is_full(&self) -> bool;
    fn spawn_food(&mut self);
    fn handle_death(&mut self, snake_id: i32);
    fn change_snake_to_zombie(&mut self, player_id: i32);
    fn config(&self) -> &GameConfig;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Pos {
    x: i32,
    y: i32,
}

#[derive(Clone, Debug)]
struct SnakeModel {
    player_id: i32,
    body: Vec<Pos>,
    state: SnakeState,
    head_direction: Direction,
}

pub struct FieldImpl {
    config: GameConfig,
    width: i32,
    height: i32,
    snakes: HashMap<i32, SnakeModel>,
    foods: HashSet<Pos>,
    players: GamePlayers,
    state_order: i32,
    rng: StdRng,
}

impl GameField for FieldImpl {
    fn new(config: GameConfig, mut players: GamePlayers) -> Self {
        let width = config.width.unwrap_or(40);
        let height = config.height.unwrap_or(30);
        let mut snakes = HashMap::new();
        let foods = HashSet::new();
        let mut rng = StdRng::from_rng(&mut rand::rng());

        // Создаем начальную змейку для первого игрока
        if !players.players.is_empty() {
            let player = &mut players.players[0];
            let head = Pos {
                x: width / 2,
                y: height / 2,
            };
            let tail_dir = Self::random_direction(&mut rng);
            let tail = Self::step(head, Self::opposite_dir(tail_dir), width, height);
            let snake = SnakeModel {
                player_id: player.id,
                body: vec![head, tail],
                state: SnakeState::Alive,
                head_direction: tail_dir,
            };
            snakes.insert(player.id, snake);
        }

        let mut field = Self {
            config,
            width,
            height,
            snakes,
            foods,
            players,
            state_order: 0,
            rng,
        };

        field.spawn_food();
        field
    }

    fn update(&mut self, steers: HashMap<i32, Direction>) -> Result<GameState> {
        self.state_order += 1;

        // Применяем команды поворота
        for (id, dir) in steers {
            if let Some(snake) = self.snakes.get_mut(&id) {
                if snake.state == SnakeState::Alive
                    && dir != Self::opposite_dir(snake.head_direction)
                {
                    snake.head_direction = dir;
                }
            }
        }

        // Вычисление новых позиций голов
        let mut intended_heads: HashMap<i32, Pos> = HashMap::new();
        for (id, snake) in &self.snakes {
            let head = snake.body[0];
            intended_heads.insert(
                *id,
                Self::step(head, snake.head_direction, self.width, self.height),
            );
        }

        // Определение съеденной еды
        let mut will_eat: HashMap<i32, bool> = HashMap::new();
        let mut eaten_food_positions: HashSet<Pos> = HashSet::new();
        for (id, pos) in &intended_heads {
            if self.foods.contains(pos) {
                will_eat.insert(*id, true);
                eaten_food_positions.insert(*pos);
            } else {
                will_eat.insert(*id, false);
            }
        }

        // Построение новых тел змеек
        let mut moved_bodies: HashMap<i32, Vec<Pos>> = HashMap::new();
        for (id, snake) in &self.snakes {
            let new_head = *intended_heads.get(id).expect("head exists");
            let eats = *will_eat.get(id).unwrap_or(&false);

            let mut new_body = Vec::with_capacity(snake.body.len() + if eats { 1 } else { 0 });
            new_body.push(new_head);
            new_body.extend_from_slice(&snake.body);
            if !eats {
                new_body.pop();
            }
            moved_bodies.insert(*id, new_body);
        }

        // Обнаружение коллизий
        let mut head_positions: HashMap<Pos, Vec<i32>> = HashMap::new();
        let mut body_owner: HashMap<Pos, i32> = HashMap::new();
        for (id, body) in &moved_bodies {
            let head = body[0];
            head_positions.entry(head).or_default().push(*id);
            for segment in body.iter().skip(1) {
                body_owner.entry(*segment).or_insert(*id);
            }
        }

        let mut to_die: HashSet<i32> = HashSet::new();

        // Если несколько голов в одной клетке - все умирают
        for (_pos, ids) in &head_positions {
            if ids.len() > 1 {
                for id in ids {
                    to_die.insert(*id);
                }
            }
        }

        let mut self_crash: HashMap<i32, bool> = HashMap::new();

        // Проверка столкновений с сами собой и с другими змеями
        for (id, body) in &moved_bodies {
            let head = body[0];
            let crashed_into_body = body_owner.contains_key(&head);
            if crashed_into_body {
                to_die.insert(*id);
            }
            let crashed_into_self = body.iter().skip(1).any(|p: &Pos| *p == head);
            self_crash.insert(*id, crashed_into_self);
        }

        for food_pos in eaten_food_positions {
            self.foods.remove(&food_pos);
        }

        // Начисление очков
        let mut score_delta: HashMap<i32, i32> = HashMap::new();

        for (id, ate) in &will_eat {
            if *ate {
                *score_delta.entry(*id).or_insert(0) += 1;
            }
        }

        for (attacker_id, body) in &moved_bodies {
            let head = body[0];
            if let Some(victim_id) = body_owner.get(&head) {
                if victim_id != attacker_id {
                    let victim_self_crash = *self_crash.get(victim_id).unwrap_or(&false);
                    if !victim_self_crash {
                        *score_delta.entry(*victim_id).or_insert(0) += 1;
                    }
                }
            }
        }

        // Собираем клетки, занятые ВЫЖИВШИМИ
        let mut survivors_occupied: HashSet<Pos> = HashSet::new();
        for (id, body) in &moved_bodies {
            if to_die.contains(id) {
                continue;
            }
            for pos in body {
                survivors_occupied.insert(*pos);
            }
        }

        // Теперь обрабатываем мёртвых
        for dead_id in to_die.iter().copied().collect::<Vec<_>>() {
            if let Some(body) = moved_bodies.get(&dead_id) {
                for pos in body {
                    if survivors_occupied.contains(pos) {
                        continue;
                    }
                    if self.rng.random_bool(0.5) {
                        self.foods.insert(*pos);
                    }
                }
            }
            self.snakes.remove(&dead_id);
        }

        // Обновляем тела выживших
        for (id, body) in moved_bodies {
            if let Some(snake) = self.snakes.get_mut(&id) {
                snake.body = body;
            }
        }

        // Обновляем счёт игроков
        for player in &mut self.players.players {
            if let Some(delta) = score_delta.get(&player.id) {
                player.score += *delta;
            }
        }

        // Пополняем еду до нужного количества
        self.spawn_food();

        Ok(self.get_current_state())
    }

    fn place_new_snake(&mut self, player_name: String) -> Result<i32> {
        let assigned_id = self.players.players.iter().map(|p| p.id).max().unwrap_or(0) + 1;

        let occupied = self.occupied_cells();

        for cy in 0..self.height {
            for cx in 0..self.width {
                if !Self::is_5x5_snake_free(&occupied, cx, cy, self.width, self.height) {
                    continue;
                }

                let head = Pos { x: cx, y: cy };
                if self.foods.contains(&head) {
                    continue;
                }

                let mut dirs = [
                    Direction::Up,
                    Direction::Down,
                    Direction::Left,
                    Direction::Right,
                ];

                dirs.shuffle(&mut self.rng);
                for tail_dir in dirs {
                    let tail = Self::step(head, tail_dir, self.width, self.height);
                    if occupied.contains(&tail) || self.foods.contains(&tail) {
                        continue;
                    }

                    let snake = SnakeModel {
                        player_id: assigned_id,
                        body: vec![head, tail],
                        state: SnakeState::Alive,
                        head_direction: Self::opposite_dir(tail_dir),
                    };
                    self.snakes.insert(assigned_id, snake);

                    if !self.players.players.iter().any(|p| p.id == assigned_id) {
                        self.players.players.push(GamePlayer {
                            name: player_name.clone(),
                            id: assigned_id,
                            ip_address: None,
                            port: None,
                            role: NodeRole::Normal as i32,
                            r#type: None,
                            score: 0,
                        });
                    }

                    return Ok(assigned_id);
                }
            }
        }

        Err(anyhow!("No space for new snake"))
    }

    fn get_current_state(&self) -> GameState {
        GameState {
            state_order: self.state_order,
            snakes: self
                .snakes
                .values()
                .map(|s| Self::snake_to_proto(s, self.width, self.height))
                .collect(),
            foods: self.foods.iter().map(|p| Self::coord(p.x, p.y)).collect(),
            players: self.players.clone(),
        }
    }

    fn is_full(&self) -> bool {
        let occupied = self.occupied_cells();
        for cy in 0..self.height {
            for cx in 0..self.width {
                if !Self::is_5x5_snake_free(&occupied, cx, cy, self.width, self.height) {
                    continue;
                }
                let head = Pos { x: cx, y: cy };
                if self.foods.contains(&head) {
                    continue;
                }
                for tail_dir in [
                    Direction::Up,
                    Direction::Down,
                    Direction::Left,
                    Direction::Right,
                ] {
                    let tail = Self::step(head, tail_dir, self.width, self.height);
                    if !occupied.contains(&tail) && !self.foods.contains(&tail) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn spawn_food(&mut self) {
        let target = self.config.food_static.unwrap_or(1).max(0) as usize
            + self
                .snakes
                .values()
                .filter(|s| s.state == SnakeState::Alive)
                .count();

        if self.foods.len() >= target {
            return;
        }

        let occupied = self.occupied_cells();
        let mut free = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let p = Pos { x, y };
                if occupied.contains(&p) || self.foods.contains(&p) {
                    continue;
                }
                free.push(p);
            }
        }

        free.shuffle(&mut self.rng);
        for p in free {
            if self.foods.len() >= target {
                break;
            }
            self.foods.insert(p);
        }
    }

    fn handle_death(&mut self, snake_id: i32) {
        if let Some(snake) = self.snakes.remove(&snake_id) {
            for pos in snake.body {
                if self.rng.random_bool(0.5) {
                    self.foods.insert(pos);
                }
            }
        }
    }

    fn config(&self) -> &GameConfig {
        &self.config
    }

    fn change_snake_to_zombie(&mut self, player_id: i32) {
        // Изменяем состояние змейки на ZOMBIE - она продолжает двигаться сама
        if let Some(snake) = self.snakes.get_mut(&player_id) {
            snake.state = SnakeState::Zombie;
            println!("Snake of player {} changed to ZOMBIE state", player_id);
        }
        
        // Также обновляем роль игрока на VIEWER
        if let Some(player) = self.players.players.iter_mut().find(|p| p.id == player_id) {
            player.role = NodeRole::Viewer as i32;
        }
    }
}

impl FieldImpl {
    fn coord(x: i32, y: i32) -> Coord {
        Coord {
            x: Some(x),
            y: Some(y),
        }
    }

    fn snake_to_proto(s: &SnakeModel, w: i32, h: i32) -> Snake {
        let mut points = Vec::with_capacity(s.body.len());
        let head = s.body[0];
        points.push(Self::coord(head.x, head.y));

        for i in 1..s.body.len() {
            let prev = s.body[i - 1];
            let next = s.body[i];
            let delta = Self::wrapped_delta(prev, next, w, h);
            points.push(Self::coord(delta.x, delta.y));
        }

        Snake {
            player_id: s.player_id,
            points,
            state: s.state as i32,
            head_direction: s.head_direction as i32,
        }
    }

    fn wrapped_delta(from: Pos, to: Pos, w: i32, h: i32) -> Pos {
        let mut dx = to.x - from.x;
        let mut dy = to.y - from.y;

        if dx > w / 2 {
            dx -= w;
        } else if dx < -w / 2 {
            dx += w;
        }
        if dy > h / 2 {
            dy -= h;
        } else if dy < -h / 2 {
            dy += h;
        }
        Pos { x: dx, y: dy }
    }

    fn step(pos: Pos, dir: Direction, w: i32, h: i32) -> Pos {
        let mut x = pos.x;
        let mut y = pos.y;
        match dir {
            Direction::Up => y = (y - 1).rem_euclid(h),
            Direction::Down => y = (y + 1).rem_euclid(h),
            Direction::Left => x = (x - 1).rem_euclid(w),
            Direction::Right => x = (x + 1).rem_euclid(w),
        }
        Pos { x, y }
    }

    fn opposite_dir(dir: Direction) -> Direction {
        match dir {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }

    fn random_direction(rng: &mut StdRng) -> Direction {
        match rng.random_range(0..4) {
            0 => Direction::Up,
            1 => Direction::Down,
            2 => Direction::Left,
            _ => Direction::Right,
        }
    }

    fn occupied_cells(&self) -> HashSet<Pos> {
        let mut out = HashSet::new();
        for s in self.snakes.values() {
            for p in &s.body {
                out.insert(*p);
            }
        }
        out
    }

    fn is_5x5_snake_free(occupied: &HashSet<Pos>, cx: i32, cy: i32, w: i32, h: i32) -> bool {
        for dy in -2..=2 {
            for dx in -2..=2 {
                let x = (cx + dx).rem_euclid(w);
                let y = (cy + dy).rem_euclid(h);
                if occupied.contains(&Pos { x, y }) {
                    return false;
                }
            }
        }
        true
    }
}
