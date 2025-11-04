<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog';

const serverIp = ref("127.0.0.1");
const serverPort = ref("4000");

type AvailableFile = { 
  name: string; 
  size_mb: number;
  isDownloading?: boolean;
  progress?: number;
  instant?: number;
  avg?: number
  time?: number
};

type UploadFile = { 
  name: string; 
  progress: number; 
  instant: number | null; 
  avg: number 
};

const downloadFiles = ref<AvailableFile[]>([]);

const uploadQueue = ref<UploadFile[]>([]);

const logs = ref<string[]>([]);

function mockDownload(file: AvailableFile) {
  const target = downloadFiles.value.find(item => item.name === file.name);
  if (!target || target.isDownloading) {
    return;
  }

  target.isDownloading = true;
  target.progress = 0;
  target.instant = 0;
  target.avg = 0;

  invoke<string>("download_file_front", {
    serverIp: serverIp.value,
    serverPort: serverPort.value,
    fileName: file.name,
  }).then((response) => {
      writeLog(`(download_file_front) Download initiated: ${JSON.stringify(response)}`);
    })
    .catch((error) => {
      writeLog(`Error downloading: ${error}`);
      target.isDownloading = false;
    });
}

async function mockUpload() {
  const file = await open({
    multiple: false,
    directory: false,
  });

  const fileName = (file as string).split(/[/\\]/).pop() || 'unknown';

  uploadQueue.value.push({ name: fileName, progress: 0, instant: 0, avg: 0 });

  await invoke<string>("upload_file_front", { serverIp: serverIp.value, serverPort: serverPort.value, filePath: file as string })
    .then((response) => {
      const target = uploadQueue.value.find(item => item.name === fileName);
      if (target) {
        target.instant = null;
        target.progress = 100;
      }
      writeLog(`(upload_file_front) Upload initiated: ${JSON.stringify(response)}`);
    })
    .catch((error) => {
      writeLog(`Error uploading: ${error}`);
    });
  
  updateAvailableFiles()
}

function updateAvailableFiles() {
  invoke<AvailableFile[]>("get_available_files", { serverIp: serverIp.value, serverPort: serverPort.value })
    .then((response) => {
      writeLog(`(get_available_files) Available files fetched: ${JSON.stringify(response)}`);
      downloadFiles.value = response;
    })
    .catch((error) => {
      writeLog(`Error fetching available files: ${error}`);
      downloadFiles.value = [];
    });
}

type ProgressData = {
  name: string;
  progress: number;
  instant: number;
  avg: number;
};

type ProgressDataDownload = {
  name: string;
  progress: number;
  instant: number;
  avg: number;
  time: number
};

const addListeners = async () => {
  listen<ProgressData>("upload_progress", ({ payload }) => {
    console.log("Upload progress:", payload);
    const file = uploadQueue.value.find(item => item.name === payload.name);
    if (file) {
      file.progress = payload.progress;
      file.instant = payload.instant;
      file.avg = payload.avg;
    }
  });

  listen<ProgressDataDownload>("download_progress", ({ payload }) => {
    console.log("Download progress:", payload);
    const file = downloadFiles.value.find(item => item.name === payload.name);
    if (file) {
      file.progress = payload.progress;
      file.instant = payload.instant;
      file.avg = payload.avg;
      file.time = payload.time;
      file.isDownloading = payload.progress < 100;
    }
  });
}

onMounted(() => {
  updateAvailableFiles()

  addListeners()
});

function writeLog(message: string) {
  logs.value.push(`[${new Date().toLocaleTimeString()}] ${message}`);
}
</script>

<template>
  <div class="app-shell">
    <section class="connection-panel">
      <div class="connection-fields">
        <label class="field">
          <span>IP address</span>
          <input
            v-model="serverIp"
            type="text"
            placeholder="192.168.0.2"
            @blur="updateAvailableFiles"
          />
        </label>
        <label class="field">
          <span>Port</span>
          <input
            v-model="serverPort"
            type="text"
            placeholder="4000"
            @blur="updateAvailableFiles"
          />
        </label>
      </div>
    </section>

    <section class="download-panel">
      <div class="panel-header">
        <h2>Download</h2>
        <button class="ghost-button" type="button" @click="updateAvailableFiles">
          Refresh
        </button>
      </div>
      <ul class="file-list">
        <li v-for="file in downloadFiles" :key="file.name" class="file-row">
          <div class="file-info">
            <span class="file-name">{{ file.name }}</span>
            <span class="file-size">{{ file.size_mb.toFixed(2) }} MB</span>
            <span class="file-size">{{ file.time }} sec</span>
          </div>
          <div class="download-actions">
            <div v-if="file.isDownloading" class="download-progress">
              <div class="progress-info">
                <span class="progress-percentage">{{ (file.progress ?? 0).toFixed(1) }}%</span>
              </div>
              <div class="progress-bar">
                <div class="progress-fill" :style="{ width: `${Math.min(file.progress ?? 0, 100)}%` }"></div>
              </div>
              <div class="download-speed">
                <span>Instant: {{ (file.instant ?? 0).toFixed(2) }} MB/s</span>
                <span>Avg: {{ (file.avg ?? 0).toFixed(2) }} MB/s</span>
              </div>
            </div>
            <button v-else class="ghost-button" @click="mockDownload(file)">Download</button>
          </div>
        </li>
      </ul>
    </section>

    <section class="upload-panel">
      <div class="panel-header">
        <h2>Upload</h2>
      </div>
      <button class="primary-button" @click="mockUpload">Upload file</button>
      <ul class="upload-list">
        <li v-for="item in uploadQueue" :key="item.name" class="upload-row">
          <div class="upload-meta">
            <span class="file-name">{{ item.name }}</span>
            <span v-if="item.instant" class="file-speed">Speed: {{ item.instant.toFixed(2) }} MB/s</span>
            <span class="file-speed">Avg: {{ item.avg.toFixed(2) }} MB/s</span>
          </div>
          <div class="progress-bar">
            <div class="progress-fill" :style="{ width: `${item.progress}%` }"></div>
          </div>
          <span class="progress-label">{{ item.progress.toFixed(0) }}%</span>
        </li>
      </ul>
    </section>

    <section class="log-panel">
      <div class="panel-header">
        <h2>Activity Log</h2>
      </div>
      <div class="log-surface">
        <pre v-for="(entry, idx) in logs" :key="idx" class="log-entry">{{ entry }}</pre>
      </div>
    </section>
  </div>
</template>

<style scoped>
* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
  font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
}
.app-shell {
  min-height: 100vh;
  padding: 48px 24px 64px;
  background: linear-gradient(180deg, #1f1f23 0%, #20202e 100%);
  color: #e5e7eb;
  display: flex;
  flex-direction: column;
  gap: 32px;
}

.header {
  max-width: 960px;
  margin: 0 auto;
  text-align: center;
}

section {
  max-width: 960px;
  margin: 0 auto;
  width: 100%;
  background-color: rgba(31, 32, 38, 0.8);
  border: 1px solid rgba(148, 163, 184, 0.24);
  border-radius: 16px;
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.panel-header h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: #f3f4f6;
}

.connection-panel .connection-fields {
  display: grid;
  gap: 16px;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  color: #d1d5db;
  font-size: 14px;
}

.field span {
  font-weight: 500;
  letter-spacing: 0.02em;
}

input {
  background-color: rgba(15, 15, 19, 0.7);
  border: 1px solid rgba(107, 114, 128, 0.4);
  border-radius: 8px;
  padding: 10px 12px;
  color: #e5e7eb;
  font-size: 14px;
  transition: border 0.2s ease;
}

input:focus {
  outline: none;
  border-color: rgba(148, 163, 184, 0.8);
}

.file-list,
.upload-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.file-row,
.upload-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px;
  background-color: rgba(12, 12, 16, 0.7);
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 12px;
}

.file-info {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.file-name {
  font-weight: 500;
  color: #f3f4f6;
}

.file-size,
.file-speed,
.progress-label {
  color: #6b7280;
  font-size: 13px;
}

.download-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 8px;
  min-width: 180px;
}

.download-progress {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 180px;
}

.progress-info {
  display: flex;
  justify-content: flex-end;
}

.progress-percentage {
  font-size: 13px;
  font-weight: 600;
  color: #60a5fa;
}

.download-speed {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: #9ca3af;
}

.ghost-button,
.primary-button {
  border-radius: 8px;
  padding: 10px 16px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  border: 1px solid transparent;
  transition: background-color 0.2s ease, border 0.2s ease, color 0.2s ease;
}

.ghost-button {
  background-color: transparent;
  color: #d1d5db;
  border-color: rgba(148, 163, 184, 0.35);
}

.ghost-button:hover {
  border-color: rgba(209, 213, 219, 0.6);
  color: #f9fafb;
}

.primary-button {
  align-self: flex-start;
  background-color: rgba(75, 85, 99, 0.75);
  color: #f9fafb;
}

.primary-button:hover {
  background-color: rgba(107, 114, 128, 0.85);
}

.upload-meta {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: 1;
}

.progress-bar {
  position: relative;
  height: 12px;
  flex: 1;
  background-color: rgba(31, 41, 55, 0.9);
  border: 1px solid rgba(148, 163, 184, 0.2);
  border-radius: 999px;
  overflow: hidden;
  min-width: 100px;
}

.progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #3b82f6 0%, #60a5fa 100%);
  transition: width 0.3s ease;
  min-width: 2px;
}

.log-surface {
  background-color: rgba(12, 12, 16, 0.7);
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 12px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 220px;
  overflow-y: auto;
}

.log-entry {
  margin: 0;
  font-size: 13px;
  color: #9ca3af;
}

@media (max-width: 640px) {
  .app-shell {
    padding: 32px 16px;
    gap: 24px;
  }

  section {
    padding: 20px;
  }

  .file-row,
  .upload-row {
    flex-direction: column;
    align-items: flex-start;
  }

  .progress-bar {
    width: 100%;
  }
}
</style>