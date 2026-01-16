<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

const emit = defineEmits<{
  close: [];
}>();

const gameName = ref("");
const fieldWidth = ref(20);
const fieldHeight = ref(20);
const updateFrequency = ref(100);

async function createGame() {
  try {
    const result = await invoke("create_new_game", {
      name: gameName.value,
      width: fieldWidth.value,
      height: fieldHeight.value,
      frequency: updateFrequency.value,
    });
    console.log("Game created:", result);
    emit("close");
  } catch (error) {
    console.error("Failed to create game:", error);
  }
}
</script>

<template>
  <div class="new-game-form">
    <h3>Настройки игры</h3>
    
    <div class="form-group">
      <label>Имя игры</label>
      <input 
        v-model="gameName" 
        type="text" 
        placeholder="Моя игра" 
        maxlength="50"
      />
    </div>
    
    <div class="form-row">
      <div class="form-group">
        <label>Ширина поля</label>
        <input 
          v-model.number="fieldWidth" 
          type="number" 
          min="10" 
          max="100" 
        />
      </div>
      
      <div class="form-group">
        <label>Высота поля</label>
        <input 
          v-model.number="fieldHeight" 
          type="number" 
          min="10" 
          max="100" 
        />
      </div>
    </div>
    
    <div class="form-group">
      <label>Частота обновления (мс)</label>
      <input 
        v-model.number="updateFrequency" 
        type="number" 
        min="50" 
        max="1000" 
        step="50" 
      />
    </div>
    
    <button class="create-button" @click="createGame">
      Создать игру
    </button>
  </div>
</template>

<style scoped>
.new-game-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

h3 {
  margin: 0;
  color: #e2e8f0;
  font-size: 18px;
  font-weight: 600;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: 1;
}

.form-row {
  display: flex;
  gap: 12px;
}

label {
  color: #cbd5e0;
  font-size: 14px;
  font-weight: 500;
}

input {
  padding: 10px 12px;
  border: 1px solid #4a5568;
  border-radius: 4px;
  background: #2d3748;
  color: #e2e8f0;
  font-size: 14px;
  transition: all 0.2s ease;
}

input:focus {
  outline: none;
  border-color: #3182ce;
  background: #354154;
}

input::placeholder {
  color: #718096;
}

input[type="number"] {
  -moz-appearance: textfield;
}

input[type="number"]::-webkit-inner-spin-button,
input[type="number"]::-webkit-outer-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.create-button {
  padding: 12px 24px;
  font-size: 15px;
  font-weight: 600;
  color: white;
  background: #3182ce;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s ease;
  margin-top: 8px;
}

.create-button:hover {
  background: #2c5282;
}

.create-button:active {
  transform: scale(0.98);
}
</style>
