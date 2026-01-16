<script setup lang="ts">
import { ref } from "vue";
import MainMenu from "./components/MainMenu.vue";
import GameView from "./components/GameView.vue";

const currentView = ref<"menu" | "game">("menu");
const currentGameName = ref<string>("");

function startGame(gameName: string) {
  currentGameName.value = gameName;
  currentView.value = "game";
}

function returnToMenu() {
  currentView.value = "menu";
}

(window as any).startGame = startGame;
</script>

<template>
  <main>
    <MainMenu v-if="currentView === 'menu'" />
    <GameView v-else-if="currentView === 'game'" @leave="returnToMenu" />
  </main>
</template>

<style scoped>
main {
  width: 100%;
  height: 100%;
}
</style>

<style>
:root {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  font-size: 16px;
  line-height: 1.5;
  font-weight: 400;

  color: #e2e8f0;
  background: #0f172a;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

body {
  margin: 0;
  min-height: 100vh;
}

#app {
  min-height: 100vh;
}
</style>
