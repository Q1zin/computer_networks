<script setup lang="ts">
import MainMenu from "./components/MainMenu.vue";
</script>

<template>
  <main>
    <MainMenu />
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
