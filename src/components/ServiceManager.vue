<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from "vue";
import type { ServiceDef, ServiceState } from "../types";
import {
  applyAllServices,
  captureRuntimeEnv,
  isTauri,
  loadServiceConfigs,
  pickPrograms,
  queryServiceStatus,
  registerService,
  restartService,
  saveServiceConfigs,
  startService,
  stopService,
  unregisterService,
} from "../services/shortcutService";

const services = ref<ServiceDef[]>([]);
const selectedId = ref<string>("");
const editing = ref<ServiceDef>(emptyDef());
const loading = ref(false);
const working = ref(false);
const refreshingStatus = ref(false);
const notice = ref<{ text: string; tone: "ok" | "warn" } | null>(null);
const statusMap = ref<Record<string, ServiceState>>({});
const configPath = ref("");
const useCurrentEnv = ref(true);
// 记录上一次自动填的工作目录 / 日志文件，用于判断用户是否手动改过
let autoWorkingDir = "";
let autoLogFile = "";

function emptyDef(): ServiceDef {
  return {
    id: "",
    name: "",
    displayName: "",
    exePath: "",
    args: "",
    workingDir: "",
    autostart: true,
    restartOnCrash: true,
    restartDelaySecs: 5,
    logFile: "",
    envOverride: {},
  };
}

function statusLabel(state: ServiceState): string {
  switch (state) {
    case "running":
      return "运行中";
    case "stopped":
      return "已停止";
    case "start_pending":
      return "启动中";
    case "stop_pending":
      return "停止中";
    case "paused":
      return "已暂停";
    case "not_installed":
      return "未注册";
    default:
      return "未知";
  }
}

function statusTone(state: ServiceState): "ok" | "warn" | "muted" {
  if (state === "running") return "ok";
  if (state === "not_installed") return "muted";
  return "warn";
}

function makeId(): string {
  return `svc-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

function setWarn(text: string) {
  notice.value = { text, tone: "warn" };
}
function setOk(text: string) {
  notice.value = { text, tone: "ok" };
}

async function refreshStatuses() {
  if (!isTauri()) {
    statusMap.value = {};
    return;
  }
  refreshingStatus.value = true;
  try {
    const next: Record<string, ServiceState> = {};
    for (const svc of services.value) {
      next[svc.name] = await queryServiceStatus(svc.name);
    }
    statusMap.value = next;
  } catch (error) {
    setWarn(error instanceof Error ? error.message : String(error));
  } finally {
    refreshingStatus.value = false;
  }
}

async function reload() {
  loading.value = true;
  try {
    const config = await loadServiceConfigs();
    services.value = config.services;
    if (!selectedId.value && services.value.length) {
      select(services.value[0].id);
    } else if (selectedId.value && !services.value.some((s) => s.id === selectedId.value)) {
      selectedId.value = "";
    }
    await refreshStatuses();
  } catch (error) {
    setWarn(error instanceof Error ? error.message : String(error));
  } finally {
    loading.value = false;
  }
}

function select(id: string) {
  selectedId.value = id;
  const svc = services.value.find((s) => s.id === id);
  if (svc) {
    editing.value = JSON.parse(JSON.stringify(svc));
    useCurrentEnv.value = Boolean(svc.envOverride && Object.keys(svc.envOverride).length > 0);
    autoWorkingDir = svc.workingDir || "";
    autoLogFile = svc.logFile || "";
  }
}

function startCreate() {
  selectedId.value = "";
  editing.value = emptyDef();
  useCurrentEnv.value = true;
  autoWorkingDir = "";
  autoLogFile = "";
}

function validate(def: ServiceDef): string | null {
  if (!def.name.trim()) return "请填写服务名（不能含空格）";
  if (/\s/.test(def.name.trim())) return "服务名不能含空格";
  if (!def.displayName.trim()) return "请填写显示名";
  if (!def.exePath.trim()) return "请填写目标 exe 路径";
  return null;
}

async function saveConfig() {
  const def = editing.value;
  const problem = validate(def);
  if (problem) {
    setWarn(problem);
    return;
  }
  working.value = true;
  try {
    if (!def.id) def.id = makeId();
    if (useCurrentEnv.value) {
      def.envOverride = await captureRuntimeEnv();
    } else {
      def.envOverride = {};
    }
    const idx = services.value.findIndex((s) => s.id === def.id);
    if (idx >= 0) services.value[idx] = JSON.parse(JSON.stringify(def));
    else services.value.push(JSON.parse(JSON.stringify(def)));
    selectedId.value = def.id;
    const path = await saveServiceConfigs(services.value);
    configPath.value = path;
    setOk(`配置已保存到 ${path}`);
    await refreshStatuses();
  } catch (error) {
    setWarn(error instanceof Error ? error.message : String(error));
  } finally {
    working.value = false;
  }
}

function exeDirOf(exePath: string): string {
  const norm = exePath.replace(/\//g, "\\").replace(/[\\]+$/, "");
  const idx = norm.lastIndexOf("\\");
  return idx >= 0 ? norm.slice(0, idx) : "";
}

function exeStemOf(exePath: string): string {
  const norm = exePath.replace(/\//g, "\\");
  const base = norm.slice(norm.lastIndexOf("\\") + 1);
  return base.replace(/\.exe$/i, "") || "app";
}

/**
 * 用户切换目标 exe 时，自动把工作目录 / 日志文件默认到 exe 所在目录；
 * 但如果用户已经手动改过这两个字段（不再等于上一次自动填的值），就不覆盖。
 */
function applyDefaultPaths(exePath: string) {
  if (!exePath.trim()) return;
  const dir = exeDirOf(exePath);
  const def = editing.value;
  const isDefaultWorking = !def.workingDir.trim() || def.workingDir === autoWorkingDir;
  const isDefaultLog = !def.logFile.trim() || def.logFile === autoLogFile;
  if (isDefaultWorking && dir) {
    def.workingDir = dir;
  }
  if (isDefaultLog && dir) {
    def.logFile = `${dir}\\${exeStemOf(exePath)}.log`;
  }
  autoWorkingDir = def.workingDir;
  autoLogFile = def.logFile;
}

async function pickExe() {
  try {
    const picked = await pickPrograms();
    if (picked.length) {
      editing.value.exePath = picked[0].path;
      applyDefaultPaths(editing.value.exePath);
      if (!editing.value.displayName.trim()) {
        editing.value.displayName = picked[0].name.replace(/\.exe$/i, "");
      }
    }
  } catch (error) {
    setWarn(error instanceof Error ? error.message : String(error));
  }
}

async function withAction(label: string, fn: () => Promise<unknown>) {
  working.value = true;
  notice.value = null;
  try {
    await fn();
    setOk(label);
  } catch (error) {
    setWarn(`${label}失败：${error instanceof Error ? error.message : String(error)}`);
  } finally {
    working.value = false;
    await refreshStatuses();
  }
}

async function registerCurrent() {
  await saveConfig();
  if (!editing.value.id) return;
  const svc = services.value.find((s) => s.id === editing.value.id);
  if (!svc) return;
  await withAction(`已注册服务「${svc.displayName}」`, () => registerService(svc));
}

async function startCurrent() {
  const svc = editing.value;
  await withAction(`已启动「${svc.displayName}」`, () => startService(svc.name));
}

async function stopCurrent() {
  const svc = editing.value;
  await withAction(`已停止「${svc.displayName}」`, () => stopService(svc.name));
}

async function restartCurrent() {
  const svc = editing.value;
  await withAction(`已重启「${svc.displayName}」`, () => restartService(svc.name));
}

async function unregisterCurrent() {
  const svc = editing.value;
  if (!window.confirm(`从系统服务里卸载「${svc.displayName}」？配置文件会保留，下次可以重新注册。`)) return;
  await withAction(`已卸载「${svc.displayName}」`, () => unregisterService(svc.name));
}

async function applyAll() {
  working.value = true;
  notice.value = null;
  try {
    const results = await applyAllServices();
    setOk(results.join("；"));
  } catch (error) {
    setWarn(error instanceof Error ? error.message : String(error));
  } finally {
    working.value = false;
    await refreshStatuses();
  }
}

let timer: number | undefined;
watch(
  () => editing.value.exePath,
  (newPath, oldPath) => {
    if (newPath && newPath !== oldPath) applyDefaultPaths(newPath);
  }
);
onMounted(() => {
  void reload();
  if (isTauri()) {
    timer = window.setInterval(() => void refreshStatuses(), 8000);
  }
});
onBeforeUnmount(() => {
  if (timer) window.clearInterval(timer);
});
</script>

<template>
  <section class="svc-page">
    <div class="svc-header">
      <div class="svc-title">
        <h2>服务管理</h2>
        <p>把任意 exe + 参数注册成 Windows 服务，配置保存在 WinKit-services.json，重装系统后一键恢复。</p>
      </div>
      <div class="svc-header-actions">
        <button class="btn" type="button" :disabled="working || loading" @click="reload">刷新状态</button>
        <button class="btn btn-primary" type="button" :disabled="working || !services.length" @click="applyAll">
          全部注册并启动
        </button>
      </div>
    </div>

    <p v-if="notice" class="svc-notice" :class="notice.tone">
      {{ notice.text }}
    </p>
    <p v-else-if="!isTauri()" class="svc-notice warn">
      浏览器预览：只展示示例配置，注册 / 启停服务需要在桌面应用里操作。
    </p>

    <div class="svc-body">
      <aside class="svc-list">
        <div
          v-for="svc in services"
          :key="svc.id"
          class="svc-item"
          :class="{ active: svc.id === selectedId }"
          @click="select(svc.id)"
        >
          <div class="svc-item-row">
            <span class="svc-item-name">{{ svc.displayName || svc.name }}</span>
            <span class="svc-badge" :class="statusTone(statusMap[svc.name] ?? 'unknown')">
              {{ statusLabel(statusMap[svc.name] ?? "unknown") }}
            </span>
          </div>
          <div class="svc-item-sub">{{ svc.name }}</div>
        </div>
        <button class="svc-add" type="button" @click="startCreate">＋ 新增服务</button>
      </aside>

      <div class="svc-detail">
        <template v-if="editing.id || selectedId === ''">
          <div class="svc-field">
            <label>服务名（SCM 名，不能含空格）</label>
            <input v-model="editing.name" placeholder="例如 WinKit-bililive" />
          </div>
          <div class="svc-field">
            <label>显示名</label>
            <input v-model="editing.displayName" placeholder="例如 B站直播录制" />
          </div>
          <div class="svc-field">
            <label>目标程序</label>
            <div class="svc-row">
              <input v-model="editing.exePath" placeholder="D:\\path\\to\\app.exe" />
              <button class="btn" type="button" @click="pickExe">选择…</button>
            </div>
          </div>
          <div class="svc-field">
            <label>命令行参数（原样拼接）</label>
            <input v-model="editing.args" placeholder='--config=D:\path\to\config.yml' />
          </div>
          <div class="svc-field">
            <label>工作目录（留空自动用目标 exe 所在目录）</label>
            <input v-model="editing.workingDir" placeholder="D:\\path\\to" />
          </div>
          <div class="svc-field">
            <label>日志文件（留空自动写到目标 exe 同目录 &lt;exe名&gt;.log）</label>
            <input v-model="editing.logFile" placeholder="D:\\path\\to\\service.log" />
          </div>
          <div class="svc-checks">
            <label><input type="checkbox" v-model="editing.autostart" /> 开机自启</label>
            <label><input type="checkbox" v-model="editing.restartOnCrash" /> 崩溃后自动重启</label>
            <label v-if="editing.restartOnCrash" class="svc-delay">
              重启间隔
              <input v-model.number="editing.restartDelaySecs" type="number" min="1" max="3600" />
              秒
            </label>
            <label title="以 LocalSystem 身份运行时，子进程会拿到快照下来的 USERPROFILE / PATH / APPDATA，避免 ffmpeg 找不到、cookie 目录错到 systemprofile">
              <input type="checkbox" v-model="useCurrentEnv" /> 使用当前用户环境运行
            </label>
          </div>

          <div class="svc-actions">
            <button class="btn btn-primary" type="button" :disabled="working" @click="saveConfig">
              保存配置
            </button>
            <button
              class="btn btn-primary"
              type="button"
              :disabled="working || !editing.id"
              @click="registerCurrent"
            >
              注册 / 更新服务
            </button>
            <button class="btn" type="button" :disabled="working || !editing.id" @click="startCurrent">启动</button>
            <button class="btn" type="button" :disabled="working || !editing.id" @click="stopCurrent">停止</button>
            <button class="btn" type="button" :disabled="working || !editing.id" @click="restartCurrent">重启</button>
            <button class="btn svc-danger" type="button" :disabled="working || !editing.id" @click="unregisterCurrent">
              卸载服务
            </button>
          </div>
          <p v-if="configPath" class="svc-file">配置文件：{{ configPath }}</p>
        </template>
        <div v-else class="svc-empty">
          <p>从左侧选一个服务，或点「＋ 新增服务」开始。</p>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.svc-page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  background: #fff;
  border: 1px solid var(--line);
  border-radius: 16px;
  padding: 18px 20px;
}

.svc-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.svc-title h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  color: var(--text);
}

.svc-title p {
  margin: 4px 0 0;
  font-size: 12.5px;
  color: var(--text-2);
}

.svc-header-actions {
  display: flex;
  gap: 8px;
  flex: none;
}

.svc-notice {
  margin: 0;
  padding: 9px 12px;
  border-radius: 10px;
  font-size: 12.5px;
  background: var(--accent-soft);
  color: var(--accent-strong);
}

.svc-notice.warn {
  background: #fdf2e9;
  color: #b25b13;
}

.svc-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 16px;
}

.svc-list {
  width: 280px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow: auto;
  padding-right: 4px;
}

.svc-item {
  border: 1px solid transparent;
  border-radius: 10px;
  padding: 10px 12px;
  cursor: pointer;
}

.svc-item:hover {
  background: #f4f8fc;
}

.svc-item.active {
  background: #eaf3fc;
  border-color: #c9dcef;
}

.svc-item-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.svc-item-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}

.svc-item-sub {
  margin-top: 2px;
  font-size: 11px;
  color: var(--text-3);
}

.svc-badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: #eef2f6;
  color: var(--text-3);
}

.svc-badge.ok {
  background: #e6f6ec;
  color: #177a3c;
}

.svc-badge.warn {
  background: #fdf2e9;
  color: #b25b13;
}

.svc-badge.muted {
  background: #eef2f6;
  color: var(--text-3);
}

.svc-add {
  margin-top: 4px;
  height: 34px;
  border: 1px dashed #c9dcef;
  border-radius: 10px;
  background: transparent;
  color: var(--accent-strong);
  font-size: 12.5px;
  cursor: pointer;
}

.svc-add:hover {
  background: #f4f9fe;
}

.svc-detail {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow: auto;
  padding: 4px 4px 4px 6px;
}

.svc-field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.svc-field label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-3);
}

.svc-field input {
  height: 32px;
  padding: 0 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  font-size: 12.5px;
  background: #fff;
  color: var(--text);
}

.svc-field input:focus {
  outline: 2px solid var(--accent-soft);
  border-color: var(--accent);
}

.svc-row {
  display: flex;
  gap: 8px;
}

.svc-row input {
  flex: 1;
}

.svc-checks {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 14px;
  font-size: 12.5px;
  color: var(--text-2);
}

.svc-delay {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.svc-delay input {
  width: 64px;
  height: 26px;
  padding: 0 6px;
  border: 1px solid var(--line);
  border-radius: 6px;
}

.svc-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 4px;
}

.svc-danger {
  color: #b42318;
  border-color: #f1c2bd;
}

.svc-danger:hover {
  background: #fdf1f0;
}

.svc-file {
  margin: 0;
  font-size: 11.5px;
  color: var(--text-3);
}

.svc-empty {
  margin: auto;
  color: var(--text-3);
  font-size: 13px;
}
</style>
