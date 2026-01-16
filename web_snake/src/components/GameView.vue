<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import GameField from "./GameField.vue";
import PlayersList from "./PlayersList.vue";
import type { GameState } from "../types/game";
import { Direction } from "../types/game";

const emit = defineEmits<{
  leave: [];
}>();

const gameState = ref<GameState>({
  stateOrder: 0,
  snakes: [],
  foods: [],
  players: {
    players: [],
  },
  config: {
    width: 40,
    height: 30,
    foodStatic: 1,
    stateDelayMs: 1000,
  },
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
    let res = await invoke("send_steer", { direction });
    console.log("Sent direction:", res);
  } catch (error) {
    console.error("Failed to send direction:", error);
  }
}

async function leaveGame() {
  try {
    let res = await invoke("leave_game");
    console.log("Left game:", res);
    emit("leave");
  } catch (error) {
    console.error("Failed to leave game:", error);
  }
}

async function becomeSpectator() {
  try {
    let res = await invoke("become_spectator");
    console.log("Became spectator:", res);
  } catch (error) {
    console.error("Failed to become spectator:", error);
  }
}

let unlistenGameState: (() => void) | null = null;

onMounted(async () => {
  window.addEventListener("keydown", handleKeyPress);
  
  unlistenGameState = await listen<GameState>("game-state", (event) => {
    console.log("Received game state:", event.payload);
    gameState.value = event.payload;
    gameFieldRef.value?.drawGame();
  });
  
  gameFieldRef.value?.drawGame();
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleKeyPress);
  
  if (unlistenGameState) {
    unlistenGameState();
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
          :config="gameState.config" 
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
            <span class="stat-value">{{ gameState.config.width }}×{{ gameState.config.height }}</span>
          </div>
          <div class="stat-item">
            <span class="stat-label">Задержка:</span>
            <span class="stat-value">{{ gameState.config.stateDelayMs }}мс</span>
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
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow: hidden;
}

.game-hint {
  text-align: center;
  color: #a0aec0;
  font-size: 14px;
}

.game-sidebar {
  width: 300px;
  flex-shrink: 0;
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
