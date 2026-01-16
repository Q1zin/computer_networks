<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";

interface Props {
  name: string;
  playerCount: number;
  isSelected: boolean;
}

const props = defineProps<Props>();
const emit = defineEmits<{
  select: [name: string];
}>();

async function joinAsPlayer() {
  try {
    const result = await invoke("join_game_as_player", {
      gameName: props.name,
    });
    console.log("Joined as player:", result);
  } catch (error) {
    console.error("Failed to join as player:", error);
  }
}

async function joinAsSpectator() {
  try {
    const result = await invoke("join_game_as_spectator", {
      gameName: props.name,
    });
    console.log("Joined as spectator:", result);
  } catch (error) {
    console.error("Failed to join as spectator:", error);
  }
}
</script>

<template>
  <div 
    class="game-item"
    :class="{ selected: isSelected }"
    @click="emit('select', name)"
  >
    <div class="game-info">
      <div class="game-details">
        <strong class="game-name">{{ name }}</strong>
        <span class="player-count">{{ playerCount }} игроков</span>
      </div>
      <div class="game-actions">
        <button 
          class="join-btn player-btn" 
          @click.stop="joinAsPlayer"
          title="Присоединиться как игрок"
        >
          Игрок
        </button>
        <button 
          class="join-btn spectator-btn" 
          @click.stop="joinAsSpectator"
          title="Присоединиться как зритель"
        >
          Зритель
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.game-item {
  padding: 12px 16px;
  margin-bottom: 8px;
  background: #2a2a2a;
  border: 2px solid #3a3a3a;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.game-item:hover {
  background: #323232;
  border-color: #4a4a4a;
}

.game-item.selected {
  background: #2d3748;
  border-color: #4a5568;
}

.game-info {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
}

.game-details {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.game-name {
  font-size: 16px;
  color: #e2e8f0;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.player-count {
  font-size: 13px;
  color: #a0aec0;
}

.game-actions {
  display: flex;
  gap: 8px;
}

.join-btn {
  padding: 6px 14px;
  font-size: 13px;
  font-weight: 500;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
}

.player-btn {
  background: #3182ce;
  color: white;
}

.player-btn:hover {
  background: #2c5282;
}

.spectator-btn {
  background: #718096;
  color: white;
}

.spectator-btn:hover {
  background: #4a5568;
}

.join-btn:active {
  transform: scale(0.95);
}
</style>
