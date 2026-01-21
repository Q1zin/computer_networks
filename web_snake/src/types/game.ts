export enum Direction {
  UP = 1,
  DOWN = 2,
  LEFT = 3,
  RIGHT = 4,
}

export enum NodeRole {
  NORMAL = 0,
  MASTER = 1,
  DEPUTY = 2,
  VIEWER = 3,
}

export enum PlayerType {
  HUMAN = 0,
  ROBOT = 1,
}

export enum SnakeState {
  ALIVE = 0,
  ZOMBIE = 1,
}

export interface Coord {
  x: number;
  y: number;
}

export interface Snake {
  playerId: number;
  points: Coord[];
  state: SnakeState;
  headDirection: Direction;
}

export interface GamePlayer {
  name: string;
  id: number;
  ipAddress?: string;
  port?: number;
  role: NodeRole;
  type: PlayerType;
  score: number;
}

export interface GameConfig {
  width: number;
  height: number;
  foodStatic: number;
  stateDelayMs: number;
}

export interface GamePlayers {
  players: GamePlayer[];
}

export interface GameState {
  stateOrder: number;
  snakes: Snake[];
  foods: Coord[];
  players: GamePlayers;
  config: GameConfig;
}
