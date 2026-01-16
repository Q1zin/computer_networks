<script setup lang="ts">
import { computed } from "vue";
import type { GamePlayer } from "../types/game";
import { NodeRole } from "../types/game";

interface Props {
  players: GamePlayer[];
  currentPlayerId?: number;
}

const props = defineProps<Props>();

function getRoleName(role: NodeRole): string {
  switch (role) {
    case NodeRole.MASTER:
      return "Главный";
    case NodeRole.DEPUTY:
      return "Заместитель";
    case NodeRole.VIEWER:
      return "Зритель";
    case NodeRole.NORMAL:
      return "Игрок";
    default:
      return "Неизвестно";
  }
}

function getRoleColor(role: NodeRole): string {
  switch (role) {
    case NodeRole.MASTER:
      return "#f59e0b";
    case NodeRole.DEPUTY:
      return "#3b82f6";
    case NodeRole.VIEWER:
      return "#718096";
    case NodeRole.NORMAL:
      return "#10b981";
    default:
      return "#6b7280";
  }
}

const sortedPlayers = computed(() => {
  return [...props.players].sort((a, b) => b.score - a.score);
});
</script>

<template>
  <div class="players-list">
    <h3>Игроки</h3>
    <div class="players-container">
      <div
        v-for="player in sortedPlayers"
        :key="player.id"
        class="player-item"
        :class="{ current: player.id === currentPlayerId }"
      >
        <div class="player-info">
          <div class="player-header">
            <span class="player-name">{{ player.name }}</span>
            <span 
              class="player-role" 
              :style="{ color: getRoleColor(player.role) }"
            >
              {{ getRoleName(player.role) }}
            </span>
          </div>
          <div class="player-score">{{ player.score }} очков</div>
        </div>
      </div>
      
      <div v-if="players.length === 0" class="no-players">
        Нет игроков
      </div>
    </div>
  </div>
</template>

<style scoped>
.players-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

h3 {
  margin: 0;
  color: #e2e8f0;
  font-size: 18px;
  font-weight: 600;
}

.players-container {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.player-item {
  background: #2a2a2a;
  border: 2px solid #3a3a3a;
  border-radius: 6px;
  padding: 12px;
  transition: all 0.2s ease;
}

.player-item.current {
  background: #2d3748;
  border-color: #4a5568;
}

.player-info {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.player-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
}

.player-name {
  font-size: 15px;
  font-weight: 500;
  color: #e2e8f0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.player-role {
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  white-space: nowrap;
}

.player-score {
  font-size: 14px;
  color: #a0aec0;
}

.no-players {
  text-align: center;
  padding: 24px;
  color: #718096;
  font-style: italic;
}
</style>
