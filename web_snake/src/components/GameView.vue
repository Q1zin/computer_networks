<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import GameField from "./GameField.vue";
import PlayersList from "./PlayersList.vue";
import type { GameState, GameConfig } from "../types/game";
import { Direction } from "../types/game";

const emit = defineEmits<{
  leave: [];
}>();

const gameState = ref<GameState>({
  stateOrder: 1,
  snakes: [
    {
      playerId: 0,
      points: [
        { x: 10, y: 10 },
        { x: 3, y: 0 },
        { x: 0, y: 2 },
      ],
      state: 0,
      headDirection: Direction.RIGHT,
    },
    {
      playerId: 1,
      points: [
        { x: 20, y: 15 },
        { x: -4, y: 0 },
      ],
      state: 0,
      headDirection: Direction.LEFT,
    },
    {
      playerId: 2,
      points: [
        { x: 15, y: 5 },
        { x: 0, y: 3 },
      ],
      state: 1,
      headDirection: Direction.DOWN,
    },
  ],
  foods: [
    { x: 5, y: 5 },
    { x: 25, y: 20 },
    { x: 30, y: 10 },
  ],
  players: {
    players: [
      { name: "Игрок 1", id: 0, role: 1, type: 0, score: 150 },
      { name: "Игрок 2", id: 1, role: 0, type: 0, score: 120 },
      { name: "Игрок 3", id: 2, role: 3, type: 0, score: 80 },
    ],
  },
});

const config = ref<GameConfig>({
  width: 40,
  height: 30,
  foodStatic: 3,
  stateDelayMs: 1000,
});

const currentPlayerId = ref<number>(0);
const gameFieldRef = ref<InstanceType<typeof GameField> | null>(null);

function handleKeyPress(event: KeyboardEvent) {
  let direction: Direction | null = null;

  switch (event.key) {
    case "ArrowUp":
    case "w":
    case "W":
      direction = Direction.UP;
      break;
    case "ArrowDown":
    case "s":
    case "S":
      direction = Direction.DOWN;
      break;
    case "ArrowLeft":
    case "a":
    case "A":
      direction = Direction.LEFT;
      break;
    case "ArrowRight":
    case "d":
    case "D":
      direction = Direction.RIGHT;
      break;
  }

  if (direction !== null) {
    event.preventDefault();
    sendDirection(direction);
  }
}

async function sendDirection(direction: Direction) {
  try {
    await invoke("send_steer", { direction });
  } catch (error) {
    console.error("Failed to send direction:", error);
  }
}

async function leaveGame() {
  try {
    await invoke("leave_game");
    emit("leave");
  } catch (error) {
    console.error("Failed to leave game:", error);
  }
}

async function becomeSpectator() {
  try {
    await invoke("become_spectator");
  } catch (error) {
    console.error("Failed to become spectator:", error);
  }
}

let unlistenGameState: (() => void) | null = null;


function startMockEvents() {
  let stateOrder = 1;
  const mockInterval = setInterval(() => {
    stateOrder++;
    
    gameState.value = {
      ...gameState.value,
      stateOrder,
      snakes: gameState.value.snakes.map((snake, idx) => {
        const head = snake.points[0];
        let newX = head.x;
        let newY = head.y;
        
        switch (snake.headDirection) {
          case Direction.UP:
            newY = (newY - 1 + config.value.height) % config.value.height;
            break;
          case Direction.DOWN:
            newY = (newY + 1) % config.value.height;
            break;
          case Direction.LEFT:
            newX = (newX - 1 + config.value.width) % config.value.width;
            break;
          case Direction.RIGHT:
            newX = (newX + 1) % config.value.width;
            break;
        }
        
        return {
          ...snake,
          points: [
            { x: newX, y: newY },
            ...snake.points.slice(1),
          ],
        };
      }),
      players: {
        players: gameState.value.players.players.map(p => ({
          ...p,
          score: p.score + Math.floor(Math.random() * 5),
        })),
      },
    };
    
    gameFieldRef.value?.drawGame();
  }, config.value.stateDelayMs);
  
  return () => clearInterval(mockInterval);
}

let stopMockEvents: (() => void) | null = null;

onMounted(async () => {
  window.addEventListener("keydown", handleKeyPress);
  
  unlistenGameState = await listen<GameState>("game-state", (event) => {
    gameState.value = event.payload;
    gameFieldRef.value?.drawGame();
  });
  
  stopMockEvents = startMockEvents();
  
  gameFieldRef.value?.drawGame();
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleKeyPress);
  
  if (unlistenGameState) {
    unlistenGameState();
  }
  
  if (stopMockEvents) {
    stopMockEvents();
  }
});

watch(gameState, () => {
  gameFieldRef.value?.drawGame();
}, { deep: true });
</script>

<template>
  <div class="game-view">
    <div class="game-header">
      <h2>Игра в Snake</h2>
      <div class="game-controls">
        <button class="control-btn spectator-btn" @click="becomeSpectator">
          Стать зрителем
        </button>
        <button class="control-btn leave-btn" @click="leaveGame">
          Выйти из игры
        </button>
      </div>
    </div>

    <div class="game-content">
      <div class="game-main">
        <GameField 
          ref="gameFieldRef"
          :game-state="gameState" 
          :config="config" 
        />
        <!-- <div class="game-hint">
          Управление: стрелки или WASD
        </div> -->
      </div>

      <div class="game-sidebar">
        <PlayersList 
          :players="gameState.players.players" 
          :current-player-id="currentPlayerId"
        />
        
        <div class="game-stats">
          <h3>Статистика</h3>
          <div class="stat-item">
            <span class="stat-label">Размер поля:</span>
            <span class="stat-value">{{ config.width }}×{{ config.height }}</span>
          </div>
          <div class="stat-item">
            <span class="stat-label">Задержка:</span>
            <span class="stat-value">{{ config.stateDelayMs }}мс</span>
          </div>
          <div class="stat-item">
            <span class="stat-label">Ход:</span>
            <span class="stat-value">#{{ gameState.stateOrder }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.game-view {
  display: flex;
  flex-direction: column;
  height: 100vh;
  padding: 20px;
  gap: 20px;
}

.game-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

h2 {
  margin: 0;
  color: #e2e8f0;
  font-size: 28px;
  font-weight: 700;
}

.game-controls {
  display: flex;
  gap: 12px;
}

.control-btn {
  padding: 10px 20px;
  font-size: 14px;
  font-weight: 600;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.spectator-btn {
  background: #718096;
  color: white;
}

.spectator-btn:hover {
  background: #4a5568;
}

.leave-btn {
  background: #e53e3e;
  color: white;
}

.leave-btn:hover {
  background: #c53030;
}

.control-btn:active {
  transform: scale(0.98);
}

.game-content {
  display: flex;
  gap: 20px;
  flex: 1;
  min-height: 0;
}

.game-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.game-hint {
  text-align: center;
  color: #a0aec0;
  font-size: 14px;
}

.game-sidebar {
  width: 300px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.game-stats {
  background: #1a202c;
  border: 1px solid #2d3748;
  border-radius: 8px;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.game-stats h3 {
  margin: 0;
  color: #e2e8f0;
  font-size: 18px;
  font-weight: 600;
}

.stat-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid #2d3748;
}

.stat-item:last-child {
  border-bottom: none;
}

.stat-label {
  color: #cbd5e0;
  font-size: 14px;
}

.stat-value {
  color: #e2e8f0;
  font-size: 14px;
  font-weight: 600;
}
</style>
