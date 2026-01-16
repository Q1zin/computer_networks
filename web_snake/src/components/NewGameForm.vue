<script setup lang="ts">
import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";

const emit = defineEmits<{
  close: [];
  created: [];
}>();

const gameName = ref("");
const fieldWidth = ref(20);
const fieldHeight = ref(20);
const updateFrequency = ref(500);
const errorMessage = ref("");

// Валидация параметров по ТЗ
const validationError = computed(() => {
  if (!gameName.value.trim()) {
    return "Введите имя игры";
  }
  if (fieldWidth.value < 10 || fieldWidth.value > 100) {
    return "Ширина поля должна быть от 10 до 100";
  }
  if (fieldHeight.value < 10 || fieldHeight.value > 100) {
    return "Высота поля должна быть от 10 до 100";
  }
  if (updateFrequency.value < 100 || updateFrequency.value > 3000) {
    return "Частота обновления должна быть от 100 до 3000 мс";
  }
  return null;
});

const isValid = computed(() => validationError.value === null);

async function createGame() {
  if (!isValid.value) {
    errorMessage.value = validationError.value || "";
    return;
  }
  
  errorMessage.value = "";
  
  try {
    const result = await invoke("create_new_game", {
      name: gameName.value,
      width: fieldWidth.value,
      height: fieldHeight.value,
      frequency: updateFrequency.value,
    });
    console.log("Game created:", result);
    emit("created");
  } catch (error) {
    console.error("Failed to create game:", error);
    errorMessage.value = String(error);
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
        min="100" 
        max="3000" 
        step="50" 
      />
      <span class="hint">100 - 3000 мс</span>
    </div>
    
    <div v-if="errorMessage" class="error-message">
      {{ errorMessage }}
    </div>
    
    <button 
      class="create-button" 
      :class="{ disabled: !isValid }"
      :disabled="!isValid"
      @click="createGame"
    >
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

.create-button.disabled {
  background: #4a5568;
  cursor: not-allowed;
  opacity: 0.6;
}

.create-button.disabled:hover {
  background: #4a5568;
}

.error-message {
  color: #fc8181;
  font-size: 13px;
  padding: 8px 12px;
  background: rgba(252, 129, 129, 0.1);
  border: 1px solid rgba(252, 129, 129, 0.3);
  border-radius: 4px;
}

.hint {
  color: #718096;
  font-size: 12px;
}
</style>
