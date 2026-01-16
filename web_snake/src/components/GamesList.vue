<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import GameItem from "./GameItem.vue";

interface GameInfo {
  gameName: string;
  playersCount: number;
  canJoin: boolean;
  masterAddress: string;
  masterIp?: string;
  masterPort?: number;
}

interface GamesDiscoveredPayload {
  games: GameInfo[];
}

const availableGames = ref<GameInfo[]>([]);
const selectedGame = ref<string | null>(null);

let unlisten: UnlistenFn | null = null;

async function refreshGameList() {
  try {
    let res = await invoke("search_games");
    console.log("Sent discover request ", res);
  } catch (error) {
    console.error("Failed to send discover:", error);
  }
}

function selectGame(name: string) {
  selectedGame.value = name;
}

onMounted(async () => {
  unlisten = await listen<GamesDiscoveredPayload>("games-discovered", (event) => {
    console.log("Games discovered:", event.payload);
    availableGames.value = event.payload.games;
  });

  refreshGameList();
});

onUnmounted(() => {
  if (unlisten) {
    unlisten();
  }
});
</script>

<template>
  <div class="games-list-container">
    <div class="header">
      <h3>Доступные игры</h3>
      <button class="refresh-button" @click="refreshGameList">
        ↻ Обновить
      </button>
    </div>
    
    <div class="games-list">
      <GameItem
        v-for="game in availableGames"
        :key="game.gameName"
        :name="game.gameName"
        :player-count="game.playersCount"
        :can-join="game.canJoin"
        :master-ip="game.masterIp"
        :master-port="game.masterPort"
        :is-selected="selectedGame === game.gameName"
        @select="selectGame"
      />
      
      <div v-if="availableGames.length === 0" class="no-games">
        Нет доступных игр
      </div>
    </div>
  </div>
</template>

<style scoped>
.games-list-container {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

h3 {
  margin: 0;
  color: #e2e8f0;
  font-size: 18px;
  font-weight: 600;
}

.refresh-button {
  padding: 6px 12px;
  font-size: 14px;
  color: #e2e8f0;
  background: #2d3748;
  border: 1px solid #4a5568;
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.refresh-button:hover {
  background: #3a4556;
  border-color: #5a6578;
}

.refresh-button:active {
  transform: scale(0.95);
}

.games-list {
  max-height: 400px;
  overflow-y: auto;
  padding: 4px;
}

.no-games {
  text-align: center;
  padding: 40px 20px;
  color: #718096;
  font-style: italic;
}

/* Скроллбар */
.games-list::-webkit-scrollbar {
  width: 8px;
}

.games-list::-webkit-scrollbar-track {
  background: #1a202c;
  border-radius: 4px;
}

.games-list::-webkit-scrollbar-thumb {
  background: #4a5568;
  border-radius: 4px;
}

.games-list::-webkit-scrollbar-thumb:hover {
  background: #5a6578;
}
</style>
