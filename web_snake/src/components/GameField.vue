<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import type { GameState, GameConfig, Snake, Coord } from "../types/game";

interface Props {
  gameState: GameState;
  config: GameConfig;
}

const props = defineProps<Props>();

const canvas = ref<HTMLCanvasElement | null>(null);
const cellSize = computed(() => {
  if (!canvas.value) return 20;
  const width = canvas.value.width / props.config.width;
  const height = canvas.value.height / props.config.height;
  return Math.min(width, height);
});

const playerColors = [
  "#3b82f6",
//   "#ef4444",
//   "#10b981",
//   "#f59e0b",
//   "#8b5cf6",
//   "#ec4899",
//   "#06b6d4",
//   "#84cc16",
];

function getPlayerColor(playerId: number): string {
  return playerColors[playerId % playerColors.length];
}

function drawGame() {
  if (!canvas.value) return;
  const ctx = canvas.value.getContext("2d");
  if (!ctx) return;

  const cell = cellSize.value;
  const width = props.config.width;
  const height = props.config.height;

  ctx.fillStyle = "#1a202c";
  ctx.fillRect(0, 0, canvas.value.width, canvas.value.height);

  ctx.strokeStyle = "#2d3748";
  ctx.lineWidth = 1;
  for (let x = 0; x <= width; x++) {
    ctx.beginPath();
    ctx.moveTo(x * cell, 0);
    ctx.lineTo(x * cell, height * cell);
    ctx.stroke();
  }
  for (let y = 0; y <= height; y++) {
    ctx.beginPath();
    ctx.moveTo(0, y * cell);
    ctx.lineTo(width * cell, y * cell);
    ctx.stroke();
  }

  ctx.fillStyle = "#ef4444";
  props.gameState.foods.forEach((food) => {
    const x = (food.x % width + width) % width;
    const y = (food.y % height + height) % height;
    ctx.beginPath();
    ctx.arc(
      x * cell + cell / 2,
      y * cell + cell / 2,
      cell / 3,
      0,
      Math.PI * 2
    );
    ctx.fill();
  });

  props.gameState.snakes.forEach((snake) => {
    const color = getPlayerColor(snake.playerId);
    const segments = expandSnake(snake, width, height);

    ctx.fillStyle = snake.state === 1 ? "#718096" : color;
    segments.forEach((seg, index) => {
      const x = (seg.x % width + width) % width;
      const y = (seg.y % height + height) % height;
      
      if (index === 0) {
        ctx.fillStyle = snake.state === 1 ? "#4a5568" : darkenColor(color);
        ctx.fillRect(
          x * cell + cell * 0.1,
          y * cell + cell * 0.1,
          cell * 0.8,
          cell * 0.8
        );
      } else {
        ctx.fillStyle = snake.state === 1 ? "#718096" : color;
        ctx.fillRect(
          x * cell + cell * 0.15,
          y * cell + cell * 0.15,
          cell * 0.7,
          cell * 0.7
        );
      }
    });
  });
}

function expandSnake(snake: Snake, width: number, height: number): Coord[] {
  const segments: Coord[] = [];
  let currentX = snake.points[0].x;
  let currentY = snake.points[0].y;
  
  segments.push({ x: currentX, y: currentY });

  for (let i = 1; i < snake.points.length; i++) {
    const dx = snake.points[i].x;
    const dy = snake.points[i].y;
    
    const steps = Math.abs(dx) + Math.abs(dy);
    const stepX = dx === 0 ? 0 : dx / Math.abs(dx);
    const stepY = dy === 0 ? 0 : dy / Math.abs(dy);

    for (let j = 0; j < steps; j++) {
      currentX = (currentX + stepX + width) % width;
      currentY = (currentY + stepY + height) % height;
      segments.push({ x: currentX, y: currentY });
    }
  }

  return segments;
}

function darkenColor(color: string): string {
  const hex = color.replace("#", "");
  const r = Math.max(0, parseInt(hex.substr(0, 2), 16) - 30);
  const g = Math.max(0, parseInt(hex.substr(2, 2), 16) - 30);
  const b = Math.max(0, parseInt(hex.substr(4, 2), 16) - 30);
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

onMounted(() => {
  if (canvas.value) {
    canvas.value.width = 800;
    canvas.value.height = 600;
  }
  drawGame();
});

defineExpose({ drawGame });
</script>

<template>
  <div class="game-field">
    <canvas ref="canvas" @vue:mounted="drawGame" @vue:updated="drawGame"></canvas>
  </div>
</template>

<style scoped>
.game-field {
  background: #1a202c;
  border-radius: 8px;
  border: 2px solid #2d3748;
  padding: 16px;
  display: flex;
  justify-content: center;
  align-items: center;
}

canvas {
  max-width: 100%;
  max-height: 100%;
  image-rendering: pixelated;
}
</style>
