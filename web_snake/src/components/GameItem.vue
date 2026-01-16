<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface Props {
  name: string;
  playerCount: number;
  canJoin: boolean;
  width: number;
  height: number;
  masterIp?: string;
  masterPort?: number;
  isSelected: boolean;
}

const props = defineProps<Props>();
const emit = defineEmits<{
  select: [name: string];
  joinError: [message: string];
}>();

const isJoining = ref(false);
const errorMessage = ref<string | null>(null);
let unlistenError: UnlistenFn | null = null;
let joinTimeoutId: ReturnType<typeof setTimeout> | null = null;

onMounted(async () => {
  // Слушаем ошибки при подключении
  unlistenError = await listen<string>("game-error", (event) => {
    if (isJoining.value) {
      errorMessage.value = event.payload;
      isJoining.value = false;
      emit("joinError", event.payload);
      
      // Автоматически скрываем ошибку через 5 секунд
      setTimeout(() => {
        errorMessage.value = null;
      }, 5000);
    }
  });
});

onUnmounted(() => {
  if (unlistenError) {
    unlistenError();
  }
  if (joinTimeoutId) {
    clearTimeout(joinTimeoutId);
  }
});

async function joinAsPlayer() {
  if (isJoining.value) return;
  
  isJoining.value = true;
  errorMessage.value = null;
  
  try {
    const result = await invoke("join_game_as_player", {
      gameName: props.name,
    });
    console.log("Join request sent:", result);
    
    // Ждём Ack или ErrorMsg в течение 3 секунд
    // Если получим game-state раньше — значит подключились успешно
    joinTimeoutId = setTimeout(() => {
      if (isJoining.value) {
        // Если всё ещё joining — переходим в игру (скорее всего Ack пришёл)
        isJoining.value = false;
        if ((window as any).startGame) {
          (window as any).startGame(props.name);
        }
      }
    }, 500);
    
  } catch (error) {
    console.error("Failed to join as player:", error);
    errorMessage.value = String(error);
    isJoining.value = false;
  }
}

async function joinAsSpectator() {
  if (isJoining.value) return;
  
  isJoining.value = true;
  errorMessage.value = null;
  
  try {
    const result = await invoke("join_game_as_spectator", {
      gameName: props.name,
    });
    console.log("Join as spectator request sent:", result);
    
    // Зрители всегда могут подключиться, переходим сразу
    joinTimeoutId = setTimeout(() => {
      if (isJoining.value) {
        isJoining.value = false;
        if ((window as any).startGame) {
          (window as any).startGame(props.name);
        }
      }
    }, 500);
    
  } catch (error) {
    console.error("Failed to join as spectator:", error);
    errorMessage.value = String(error);
    isJoining.value = false;
  }
}
</script>

<template>
  <div 
    class="game-item"
    :class="{ selected: isSelected, joining: isJoining }"
    @click="emit('select', name)"
  >
    <div class="game-info">
      <div class="game-details">
        <strong class="game-name">{{ name }}</strong>
        <div class="game-meta">
          <span class="player-count">{{ playerCount }} игроков</span>
          <span class="field-size">{{ width }}×{{ height }}</span>
          <span class="join-status" :class="{ 'can-join': canJoin, 'full': !canJoin }">
            {{ canJoin ? 'Можно войти' : 'Поле заполнено' }}
          </span>
        </div>
        <span v-if="masterIp && masterPort" class="server-info">
          {{ masterIp }}:{{ masterPort }}
        </span>
        <span v-if="errorMessage" class="error-message">
          {{ errorMessage }}
        </span>
      </div>
      <div class="game-actions">
        <button 
          class="join-btn player-btn" 
          :class="{ loading: isJoining }"
          :disabled="isJoining"
          @click.stop="joinAsPlayer"
          title="Присоединиться как игрок"
        >
          {{ isJoining ? '...' : 'Игрок' }}
        </button>
        <button 
          class="join-btn spectator-btn" 
          :class="{ loading: isJoining }"
          :disabled="isJoining"
          @click.stop="joinAsSpectator"
          title="Присоединиться как зритель"
        >
          {{ isJoining ? '...' : 'Зритель' }}
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

.game-item.joining {
  opacity: 0.8;
  pointer-events: none;
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

.game-meta {
  display: flex;
  gap: 12px;
  align-items: center;
  flex-wrap: wrap;
}

.player-count {
  font-size: 13px;
  color: #a0aec0;
}

.field-size {
  font-size: 13px;
  color: #a0aec0;
  font-family: monospace;
}

.join-status {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 500;
}

.join-status.can-join {
  background: #22543d;
  color: #9ae6b4;
}

.join-status.full {
  background: #742a2a;
  color: #fc8181;
}

.server-info {
  font-size: 12px;
  color: #718096;
  font-family: monospace;
}

.error-message {
  font-size: 12px;
  color: #fc8181;
  margin-top: 4px;
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
  min-width: 70px;
}

.join-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.join-btn.loading {
  background: #4a5568;
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
