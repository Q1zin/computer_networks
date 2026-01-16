<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import GameItem from "./GameItem.vue";

interface GameInfo {
  name: string;
  playerCount: number;
}

const availableGames = ref<GameInfo[]>([
  // Мок данные для демонстрации
  { name: "Быстрая игра #1", playerCount: 3 },
  { name: "Турнир профессионалов", playerCount: 8 },
  { name: "Для новичков", playerCount: 2 },
  { name: "Мега битва", playerCount: 12 },
  { name: "Вечерняя игра", playerCount: 5 },
]);

const selectedGame = ref<string | null>(null);

async function refreshGameList() {
  try {
    const games = await invoke<GameInfo[]>("get_available_games");
    availableGames.value = games;
  } catch (error) {
    console.error("Failed to refresh game list:", error);
  }
}

function selectGame(name: string) {
  selectedGame.value = name;
}
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
        :key="game.name"
        :name="game.name"
        :player-count="game.playerCount"
        :is-selected="selectedGame === game.name"
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
