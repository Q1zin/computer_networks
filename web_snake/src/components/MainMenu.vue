<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import NewGameForm from "./NewGameForm.vue";
import GamesList from "./GamesList.vue";

const showNewGameMenu = ref(false);
const showJoinGameMenu = ref(false);

function toggleNewGameMenu() {
  showNewGameMenu.value = !showNewGameMenu.value;
  showJoinGameMenu.value = false;
}

function toggleJoinGameMenu() {
  showJoinGameMenu.value = !showJoinGameMenu.value;
  showNewGameMenu.value = false;
}

function closeNewGameMenu() {
  showNewGameMenu.value = false;
}

async function exitApp() {
  try {
    await invoke("exit_app");
  } catch (error) {
    console.error("Failed to exit:", error);
  }
}
</script>

<template>
  <div class="main-menu">
    <h1>Snake Game</h1>

    <div class="menu-container">
      <!-- Начать новую игру -->
      <div class="menu-section">
        <button class="menu-button primary" @click="toggleNewGameMenu">
          Начать новую игру
        </button>
        
        <div v-if="showNewGameMenu" class="dropdown-panel">
          <NewGameForm @close="closeNewGameMenu" />
        </div>
      </div>

      <!-- Присоединиться к игре -->
      <div class="menu-section">
        <button class="menu-button secondary" @click="toggleJoinGameMenu">
          Присоединиться к игре
        </button>
        
        <div v-if="showJoinGameMenu" class="dropdown-panel">
          <GamesList />
        </div>
      </div>

      <!-- Выйти -->
      <div class="menu-section">
        <button class="menu-button danger" @click="exitApp">
          Выйти
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.main-menu {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 20px;
}

h1 {
  margin-bottom: 48px;
  color: #e2e8f0;
  font-size: 48px;
  font-weight: 700;
  letter-spacing: -0.5px;
}

.menu-container {
  display: flex;
  flex-direction: column;
  gap: 16px;
  width: 100%;
  max-width: 500px;
}

.menu-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.menu-button {
  padding: 16px 24px;
  font-size: 16px;
  font-weight: 600;
  color: white;
  border: none;
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s ease;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.menu-button:hover {
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.menu-button:active {
  transform: translateY(0);
}

.menu-button.primary {
  background: #3182ce;
}

.menu-button.primary:hover {
  background: #2c5282;
}

.menu-button.secondary {
  background: #38a169;
}

.menu-button.secondary:hover {
  background: #2f855a;
}

.menu-button.danger {
  background: #e53e3e;
}

.menu-button.danger:hover {
  background: #c53030;
}

.dropdown-panel {
  background: #1a202c;
  border: 1px solid #2d3748;
  border-radius: 8px;
  padding: 20px;
  animation: slideDown 0.2s ease;
}

@keyframes slideDown {
  from {
    opacity: 0;
    transform: translateY(-8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
