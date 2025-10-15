<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

interface MessageEvent {
  msg_type: string;
  uuid: string;
  text: string;
  timestamp: string;
}

interface StartConfig {
  ip: string;
  port: number;
  message: string;
  interface: string | null;
}

interface DeviceData {
  uuid: string;
  last_message: string;
  message_count: number;
  seconds_since_seen: number;
}

const isRunning = ref(false);
const instanceId = ref<string | null>(null);
const messages = ref<MessageEvent[]>([]);
const statusLog = ref<string[]>([]);
const sentCount = ref(0);
const activeDevices = ref<DeviceData[]>([]);

const ipAddress = ref('239.255.255.250');
const port = ref(8888);
const message = ref('Hello from GUI');
const interfaceName = ref('');
const useAutoInterface = ref(true);

const protocolVersion = ref<'IPv4' | 'IPv6'>('IPv4');

let unlisteners: UnlistenFn[] = [];
let deviceUpdateInterval: number | null = null;

const updateDeviceList = async () => {
  try {
    const devices = await invoke<DeviceData[]>('get_active_devices');
    console.log('Active devices updated:', devices.length, devices);
    activeDevices.value = devices;
  } catch (error) {
    console.error('Failed to get active devices:', error);
  }
};

onMounted(async () => {
  const unlisten1 = await listen<MessageEvent>('multicast-message', (event) => {
    messages.value.unshift(event.payload);
    if (messages.value.length > 100) {
      messages.value = messages.value.slice(0, 100);
    }
    updateDeviceList();
  });

  const unlisten2 = await listen<string>('multicast-status', (event) => {
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] ${event.payload}`);
    if (statusLog.value.length > 50) {
      statusLog.value = statusLog.value.slice(0, 50);
    }
  });

  const unlisten3 = await listen<string>('multicast-error', (event) => {
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] ERROR: ${event.payload}`);
  });

  const unlisten4 = await listen<number>('multicast-sent', (event) => {
    sentCount.value = event.payload;
  });

  unlisteners = [unlisten1, unlisten2, unlisten3, unlisten4];

  isRunning.value = await invoke<boolean>('get_status');
  instanceId.value = await invoke<string | null>('get_instance_id');
  
  await updateDeviceList();
  
  deviceUpdateInterval = window.setInterval(updateDeviceList, 2000);
});

onUnmounted(() => {
  unlisteners.forEach(fn => fn());
  if (deviceUpdateInterval !== null) {
    clearInterval(deviceUpdateInterval);
  }
});

const detectProtocol = () => {
  if (ipAddress.value.includes(':')) {
    protocolVersion.value = 'IPv6';
  } else {
    protocolVersion.value = 'IPv4';
  }
};

const setIPv4Example = () => {
  ipAddress.value = '239.255.255.250';
  port.value = 8888;
  protocolVersion.value = 'IPv4';
};

const setIPv6Example = () => {
  ipAddress.value = 'ff08::1';
  port.value = 8888;
  protocolVersion.value = 'IPv6';
};

const startMulticast = async () => {
  try {
    const config: StartConfig = {
      ip: ipAddress.value,
      port: port.value,
      message: message.value,
      interface: useAutoInterface.value || !interfaceName.value ? null : interfaceName.value,
    };

    instanceId.value = await invoke<string>('start_multicast', { config });
    isRunning.value = true;
    sentCount.value = 0;

    await updateDeviceList();
    
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] Started with ID: ${instanceId.value}`);
  } catch (error) {
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] Failed to start: ${error}`);
  }
};

const stopMulticast = async () => {
  try {
    await invoke('stop_multicast');
    isRunning.value = false;
    instanceId.value = null;
    activeDevices.value = [];
    
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] Stopped`);
  } catch (error) {
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] Failed to stop: ${error}`);
  }
};

const updateMessage = async () => {
  if (!isRunning.value) return;
  
  try {
    await invoke('update_message', { message: message.value });
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] Message updated`);
  } catch (error) {
    const timestamp = new Date().toLocaleTimeString();
    statusLog.value.unshift(`[${timestamp}] Failed to update: ${error}`);
  }
};

const clearMessages = () => {
  messages.value = [];
};

const clearStatus = () => {
  statusLog.value = [];
};
</script>

<template>
  <div class="app">
    <header>
      <h1>🌐 Multicast UDP Messenger</h1>
      <div class="status-badge" :class="{ active: isRunning }">
        {{ isRunning ? '● ACTIVE' : '○ STOPPED' }}
      </div>
    </header>

    <div class="container">
      <!-- Configuration Panel -->
      <div class="panel config-panel">
        <h2>Configuration</h2>
        
        <div class="form-group">
          <div class="label-text">Protocol</div>
          <div class="protocol-buttons">
            <button 
              @click="setIPv4Example" 
              :class="{ active: protocolVersion === 'IPv4' }"
              :disabled="isRunning"
            >
              IPv4
            </button>
            <button 
              @click="setIPv6Example" 
              :class="{ active: protocolVersion === 'IPv6' }"
              :disabled="isRunning"
            >
              IPv6
            </button>
          </div>
        </div>

        <div class="form-group">
          <label for="ip">Multicast IP Address</label>
          <input 
            id="ip"
            v-model="ipAddress" 
            @input="detectProtocol"
            :disabled="isRunning"
            placeholder="239.255.255.250 or ff08::1"
          />
          <small>{{ protocolVersion }} detected</small>
        </div>

        <div class="form-group">
          <label for="port">Port</label>
          <input 
            id="port"
            v-model.number="port" 
            type="number"
            :disabled="isRunning"
            min="1"
            max="65535"
          />
        </div>

        <div class="form-group">
          <label for="message">Message</label>
          <input 
            id="message"
            v-model="message"
            placeholder="Your message here"
          />
          <button 
            v-if="isRunning" 
            @click="updateMessage"
            class="btn-secondary"
          >
            Update Message
          </button>
        </div>

        <div class="form-group" v-if="protocolVersion === 'IPv6'">
          <label>
            <input type="checkbox" v-model="useAutoInterface" :disabled="isRunning" />
            Auto-detect interface
          </label>
          
          <input 
            v-if="!useAutoInterface"
            v-model="interfaceName"
            :disabled="isRunning"
            placeholder="e.g., en0, bridge100"
          />
        </div>

        <div class="form-group" v-if="instanceId">
          <div class="label-text">Instance ID: <code>{{ instanceId }}</code></div>
        </div>

        <div class="actions">
          <button 
            v-if="!isRunning" 
            @click="startMulticast"
            class="btn-primary"
          >
            ▶ Start
          </button>
          <button 
            v-else 
            @click="stopMulticast"
            class="btn-danger"
          >
            ■ Stop
          </button>
        </div>
      </div>

      <!-- Active Devices Panel -->
      <div class="panel devices-panel">
        <div class="panel-header">
          <h2>Active Devices ({{ activeDevices.length }})</h2>
        </div>
        
        <div class="devices-list">
          <div 
            v-for="(device, index) in activeDevices" 
            :key="device.uuid"
            class="device-item"
            :class="{
              'device-fresh': device.seconds_since_seen < 2,
              'device-stale': device.seconds_since_seen >= 5
            }"
          >
            <div class="device-header">
              <span class="device-number">Device #{{ index + 1 }}</span>
              <span class="device-time" :class="{
                'time-fresh': device.seconds_since_seen < 2,
                'time-warning': device.seconds_since_seen >= 5 && device.seconds_since_seen < 10,
                'time-stale': device.seconds_since_seen >= 10
              }">
                {{ device.seconds_since_seen < 1 ? '< 1s' : device.seconds_since_seen + 's' }} ago
              </span>
            </div>
            <div class="device-body">
              <div class="device-message">{{ device.last_message }}</div>
              <div class="device-count">Messages: {{ device.message_count }}</div>
            </div>
          </div>
          
          <div v-if="activeDevices.length === 0" class="empty-state">
            No active devices
          </div>
        </div>
      </div>

      <!-- Messages Panel -->
      <div class="panel messages-panel">
        <div class="panel-header">
          <h2>Received Messages ({{ messages.length }})</h2>
          <div class="stats">
            <span class="stat">Sent: {{ sentCount }}</span>
            <button @click="clearMessages" class="btn-small">Clear</button>
          </div>
        </div>
        
        <div class="messages-list">
          <div 
            v-for="(msg, index) in messages" 
            :key="index"
            class="message-item"
            :class="'type-' + msg.msg_type.toLowerCase()"
          >
            <div class="message-header">
              <span class="message-type">{{ msg.msg_type }}</span>
              <span class="message-time">{{ msg.timestamp }}</span>
            </div>
            <div class="message-body">
              <div class="message-text">{{ msg.text }}</div>
              <div class="message-uuid">{{ msg.uuid }}</div>
            </div>
          </div>
          
          <div v-if="messages.length === 0" class="empty-state">
            No messages received yet
          </div>
        </div>
      </div>

      <!-- Status Log Panel -->
      <div class="panel status-panel">
        <div class="panel-header">
          <h2>Status Log</h2>
          <button @click="clearStatus" class="btn-small">Clear</button>
        </div>
        
        <div class="status-list">
          <div 
            v-for="(log, index) in statusLog" 
            :key="index"
            class="status-item"
          >
            {{ log }}
          </div>
          
          <div v-if="statusLog.length === 0" class="empty-state">
            No status messages yet
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
* {
  box-sizing: border-box;
}

.app {
  min-height: 100vh;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  padding: 20px;
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
}

header {
  text-align: center;
  color: white;
  margin-bottom: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 20px;
}

h1 {
  margin: 0;
  font-size: 2.5rem;
  font-weight: 700;
}

.status-badge {
  padding: 8px 20px;
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.2);
  font-weight: 600;
  font-size: 0.9rem;
  transition: all 0.3s;
}

.status-badge.active {
  background: #10b981;
  animation: pulse 2s infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.7; }
}

.container {
  max-width: 1600px;
  margin: 0 auto;
  display: grid;
  grid-template-columns: 350px 1fr 1fr;
  grid-template-rows: 1fr 1fr;
  gap: 20px;
  height: calc(100vh - 140px);
}

.panel {
  background: white;
  border-radius: 16px;
  padding: 24px;
  box-shadow: 0 10px 40px rgba(0, 0, 0, 0.1);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.config-panel {
  grid-row: 1 / -1;
  grid-column: 1;
}

.devices-panel {
  grid-column: 2;
  grid-row: 1 / -1;
}

.messages-panel {
  grid-column: 3;
  grid-row: 1;
}

.status-panel {
  grid-column: 3;
  grid-row: 2;
}

h2 {
  margin: 0 0 20px 0;
  font-size: 1.3rem;
  color: #1f2937;
}

.form-group {
  margin-bottom: 20px;
}

.form-group label,
.form-group .label-text {
  display: block;
  margin-bottom: 8px;
  font-weight: 500;
  color: #374151;
  font-size: 0.9rem;
}

.form-group input[type="text"],
.form-group input[type="number"],
.form-group input {
  width: 100%;
  padding: 10px 14px;
  border: 2px solid #e5e7eb;
  border-radius: 8px;
  font-size: 0.95rem;
  transition: border-color 0.2s;
}

.form-group input:focus {
  outline: none;
  border-color: #667eea;
}

.form-group input:disabled {
  background: #f9fafb;
  cursor: not-allowed;
}

.form-group small {
  display: block;
  margin-top: 4px;
  color: #6b7280;
  font-size: 0.8rem;
}

.form-group code {
  background: #f3f4f6;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 0.85rem;
  color: #667eea;
  word-break: break-all;
}

.protocol-buttons {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.protocol-buttons button {
  padding: 10px;
  border: 2px solid #e5e7eb;
  background: white;
  border-radius: 8px;
  cursor: pointer;
  font-weight: 500;
  transition: all 0.2s;
}

.protocol-buttons button:hover:not(:disabled) {
  border-color: #667eea;
}

.protocol-buttons button.active {
  background: #667eea;
  color: white;
  border-color: #667eea;
}

.protocol-buttons button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.actions {
  margin-top: auto;
  padding-top: 20px;
}

button {
  cursor: pointer;
  font-weight: 600;
  border: none;
  border-radius: 8px;
  transition: all 0.2s;
  font-size: 0.95rem;
}

.btn-primary {
  width: 100%;
  padding: 14px;
  background: #10b981;
  color: white;
}

.btn-primary:hover {
  background: #059669;
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(16, 185, 129, 0.4);
}

.btn-danger {
  width: 100%;
  padding: 14px;
  background: #ef4444;
  color: white;
}

.btn-danger:hover {
  background: #dc2626;
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(239, 68, 68, 0.4);
}

.btn-secondary {
  margin-top: 8px;
  padding: 8px 16px;
  background: #667eea;
  color: white;
  font-size: 0.85rem;
}

.btn-secondary:hover {
  background: #5568d3;
}

.btn-small {
  padding: 6px 12px;
  background: #f3f4f6;
  color: #374151;
  font-size: 0.85rem;
}

.btn-small:hover {
  background: #e5e7eb;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.stats {
  display: flex;
  align-items: center;
  gap: 12px;
}

.stat {
  font-size: 0.9rem;
  color: #6b7280;
  font-weight: 500;
}

.messages-list,
.status-list,
.devices-list {
  flex: 1;
  overflow-y: auto;
  padding-right: 8px;
}

.messages-list::-webkit-scrollbar,
.status-list::-webkit-scrollbar,
.devices-list::-webkit-scrollbar {
  width: 6px;
}

.messages-list::-webkit-scrollbar-thumb,
.status-list::-webkit-scrollbar-thumb,
.devices-list::-webkit-scrollbar-thumb {
  background: #d1d5db;
  border-radius: 3px;
}

.device-item {
  padding: 12px;
  margin-bottom: 12px;
  border-left: 4px solid #10b981;
  background: #f0fdf4;
  border-radius: 8px;
  transition: all 0.3s;
}

.device-item:hover {
  transform: translateX(4px);
  box-shadow: 0 2px 8px rgba(16, 185, 129, 0.2);
}

.device-item.device-fresh {
  border-left-color: #10b981;
  background: #ecfdf5;
}

.device-item.device-stale {
  border-left-color: #f59e0b;
  background: #fffbeb;
}

.device-header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 8px;
}

.device-number {
  font-size: 0.9rem;
  color: #059669;
  font-weight: 700;
  background: #ecfdf5;
  padding: 4px 12px;
  border-radius: 6px;
}

.device-time {
  font-size: 0.75rem;
  color: #fff;
  font-weight: 600;
  padding: 3px 10px;
  border-radius: 12px;
  white-space: nowrap;
  transition: all 0.3s;
}

.device-time.time-fresh {
  background: #10b981;
  box-shadow: 0 0 8px rgba(16, 185, 129, 0.4);
}

.device-time.time-warning {
  background: #f59e0b;
}

.device-time.time-stale {
  background: #ef4444;
}

.device-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.device-message {
  color: #1f2937;
  font-size: 0.95rem;
  font-weight: 500;
  padding: 4px 0;
  border-bottom: 1px solid #e5e7eb;
}

.device-count {
  font-size: 0.75rem;
  color: #059669;
  font-weight: 600;
  background: #f0fdf4;
  padding: 2px 6px;
  border-radius: 3px;
  display: inline-block;
  margin-top: 4px;
}

.message-item {
  padding: 12px;
  margin-bottom: 12px;
  border-left: 4px solid #667eea;
  background: #f9fafb;
  border-radius: 8px;
  transition: transform 0.2s;
}

.message-item:hover {
  transform: translateX(4px);
}

.message-item.type-disconnect {
  border-left-color: #ef4444;
  background: #fef2f2;
}

.message-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.message-type {
  font-weight: 600;
  font-size: 0.85rem;
  color: #667eea;
  text-transform: uppercase;
}

.type-disconnect .message-type {
  color: #ef4444;
}

.message-time {
  font-size: 0.8rem;
  color: #9ca3af;
}

.message-body {
  font-size: 0.9rem;
}

.message-text {
  color: #1f2937;
  margin-bottom: 4px;
  font-weight: 500;
}

.message-uuid {
  font-size: 0.75rem;
  color: #9ca3af;
  font-family: 'Courier New', monospace;
}

.status-item {
  padding: 8px 12px;
  margin-bottom: 8px;
  background: #f9fafb;
  border-radius: 6px;
  font-size: 0.85rem;
  color: #374151;
  font-family: 'Courier New', monospace;
}

.empty-state {
  text-align: center;
  color: #9ca3af;
  padding: 40px 20px;
  font-size: 0.9rem;
}

input[type="checkbox"] {
  margin-right: 8px;
  cursor: pointer;
}
</style>
