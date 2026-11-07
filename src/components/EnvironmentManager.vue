<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type {
  EnvironmentProfile,
  EnvironmentProfileVariable,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
} from "../types";
import {
  deleteEnvironmentVariable,
  isTauri,
  loadEnvironmentProfiles,
  loadEnvironmentVariables,
  pickFolder,
  saveEnvironmentProfiles,
  saveEnvironmentVariable,
} from "../services/shortcutService";

type NoticeTone = "ok" | "warn";
type ProfileEditor = { id: string | null; name: string };
type VariableEditor = {
  id: string | null;
  name: string;
  value: string;
  scope: EnvironmentScope;
  expand: boolean;
  append: boolean;
};
type RowStatusKind = "loading" | "set" | "missing" | "diff" | "partial" | "error";
type RowStatus = {
  kind: RowStatusKind;
  label: string;
  current?: string;
  existingName?: string;
};

const profiles = ref<EnvironmentProfile[]>([]);
const snapshot = ref<EnvironmentSnapshot | null>(null);
const selectedId = ref<string | null>(null);
const query = ref("");
const loading = ref(true);
const saving = ref(false);
const loadError = ref("");
const notice = ref("");
const noticeTone = ref<NoticeTone>("ok");
const storePath = ref("");
const profileEditor = ref<ProfileEditor | null>(null);
const variableEditor = ref<VariableEditor | null>(null);
const profileNameInput = ref<HTMLInputElement | null>(null);
const variableNameInput = ref<HTMLInputElement | null>(null);

function setVariableNameInput(element: unknown) {
  variableNameInput.value = element instanceof HTMLInputElement ? element : null;
}

const selectedProfile = computed(() => {
  return profiles.value.find((profile) => profile.id === selectedId.value) ?? null;
});

const filteredProfiles = computed(() => {
  const keyword = query.value.trim().toLowerCase();
  if (!keyword) return profiles.value;
  return profiles.value.filter(
    (profile) =>
      profile.name.toLowerCase().includes(keyword) ||
      profile.note.toLowerCase().includes(keyword) ||
      profile.variables.some((variable) => variable.name.toLowerCase().includes(keyword))
  );
});

const isBrowserPreview = computed(() => !isTauri());
const systemWritable = computed(() => snapshot.value?.system.writable ?? false);
const isPathEditor = computed(
  () => Boolean(variableEditor.value) && isPathName(variableEditor.value?.name ?? "")
);

const selectedCounts = computed(() => {
  const variables = selectedProfile.value?.variables ?? [];
  let setCount = 0;
  let pendingCount = 0;
  let errorCount = 0;
  for (const variable of variables) {
    const status = getStatus(variable);
    if (status.kind === "set") setCount += 1;
    else if (status.kind === "error") errorCount += 1;
    else pendingCount += 1;
  }
  return {
    total: variables.length,
    setCount,
    pendingCount,
    errorCount,
  };
});

const enabledProfiles = computed(() => profiles.value.filter((profile) => profile.enabled));
const enabledVariableCount = computed(() =>
  enabledProfiles.value.reduce((total, profile) => total + profile.variables.length, 0)
);

function setNotice(text: string, tone: NoticeTone = "ok") {
  notice.value = text;
  noticeTone.value = tone;
}

function isPathName(name: string): boolean {
  return name.trim().toLowerCase() === "path";
}

function splitSegments(value: string): string[] {
  return value
    .split(";")
    .map((part) => part.trim())
    .filter(Boolean);
}

// Path 的首尾分号、连续分号只是写法差异，比较和写入前统一按非空分段规整。
function normalizePathValue(value: string): string {
  return splitSegments(value).join(";");
}

function mergePathValue(existing: string, desired: string): string {
  const current = splitSegments(existing);
  const additions = splitSegments(desired);
  for (const addition of additions) {
    if (!current.some((part) => part.toLowerCase() === addition.toLowerCase())) {
      current.push(addition);
    }
  }
  return current.join(";");
}

function findExisting(variable: EnvironmentProfileVariable): EnvironmentVariable | null {
  const scopeSnapshot = snapshot.value?.[variable.scope];
  if (!scopeSnapshot) return null;
  const name = variable.name.trim().toLowerCase();
  return scopeSnapshot.variables.find((item) => item.name.toLowerCase() === name) ?? null;
}

function getStatus(variable: EnvironmentProfileVariable): RowStatus {
  if (loading.value || !snapshot.value) {
    return { kind: "loading", label: "检测中" };
  }
  const scopeSnapshot = snapshot.value[variable.scope];
  if (scopeSnapshot.error) {
    return { kind: "error", label: "读取失败" };
  }
  const existing = findExisting(variable);
  if (!existing) {
    return { kind: "missing", label: "缺失" };
  }

  if (isPathName(variable.name)) {
    const currentSegments = splitSegments(existing.value);
    const desiredSegments = splitSegments(variable.value).filter(
      (segment, index, all) =>
        all.findIndex((part) => part.toLowerCase() === segment.toLowerCase()) === index
    );
    if (!desiredSegments.length) {
      return {
        kind: "missing",
        label: "值为空",
        current: existing.value,
        existingName: existing.name,
      };
    }
    const missing = desiredSegments.filter(
      (segment) => !currentSegments.some((part) => part.toLowerCase() === segment.toLowerCase())
    );
    if (!missing.length) {
      return {
        kind: "set",
        label: "已设置",
        current: existing.value,
        existingName: existing.name,
      };
    }
    return {
      kind: missing.length === desiredSegments.length ? "missing" : "partial",
      label:
        missing.length === desiredSegments.length ? "缺失" : `待设置 ${missing.length} 项`,
      current: existing.value,
      existingName: existing.name,
    };
  }

  if (existing.value.trim() === variable.value.trim()) {
    return {
      kind: "set",
      label: "已设置",
      current: existing.value,
      existingName: existing.name,
    };
  }
  return {
    kind: "diff",
    label: "值不同",
    current: existing.value,
    existingName: existing.name,
  };
}

function profileSummary(profile: EnvironmentProfile): string {
  if (loading.value) return "检测中";
  if (!profile.enabled) return "未启用";
  let pending = 0;
  let errors = 0;
  for (const variable of profile.variables) {
    const status = getStatus(variable);
    if (status.kind !== "set") {
      if (status.kind === "error") errors += 1;
      else pending += 1;
    }
  }
  if (errors) return `${errors} 项读取失败`;
  if (pending) return `${pending} 项待处理`;
  if (!profile.variables.length) return "还没有变量";
  return "全部已设置";
}

function profileSummaryTone(profile: EnvironmentProfile): string {
  if (loading.value) return "loading";
  if (!profile.enabled) return "empty";
  let pending = 0;
  let errors = 0;
  for (const variable of profile.variables) {
    const status = getStatus(variable);
    if (status.kind === "error") errors += 1;
    else if (status.kind !== "set") pending += 1;
  }
  if (errors) return "error";
  if (pending) return "pending";
  if (!profile.variables.length) return "empty";
  return "done";
}

function makeId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
}

function selectProfile(profile: EnvironmentProfile) {
  selectedId.value = profile.id;
}

function startCreateProfile() {
  profileEditor.value = { id: null, name: "" };
  variableEditor.value = null;
  nextTick(() => profileNameInput.value?.focus());
}

function startRenameProfile(profile: EnvironmentProfile) {
  profileEditor.value = { id: profile.id, name: profile.name };
  nextTick(() => profileNameInput.value?.focus());
}

async function saveProfileEditor() {
  if (!profileEditor.value) return;
  const name = profileEditor.value.name.trim();
  if (!name) {
    setNotice("方案名称不能为空", "warn");
    nextTick(() => profileNameInput.value?.focus());
    return;
  }
  const duplicate = profiles.value.some(
    (profile) => profile.id !== profileEditor.value?.id && profile.name.trim().toLowerCase() === name.toLowerCase()
  );
  if (duplicate) {
    setNotice("已经有同名方案了，请换一个名称", "warn");
    nextTick(() => profileNameInput.value?.focus());
    return;
  }

  if (profileEditor.value.id) {
    const profile = profiles.value.find((item) => item.id === profileEditor.value?.id);
    if (profile) profile.name = name;
    setNotice(`已修改方案名称“${name}”`);
  } else {
    const profile: EnvironmentProfile = {
      id: makeId("env-profile"),
      name,
      note: "",
      enabled: true,
      variables: [],
    };
    profiles.value.push(profile);
    selectedId.value = profile.id;
    setNotice(`已新建方案“${name}”，现在添加 key:value`);
  }
  profileEditor.value = null;
  await persistProfiles();
}

async function deleteProfile() {
  const profile = selectedProfile.value;
  if (!profile) return;
  if (
    !window.confirm(
      `删除方案“${profile.name}”？\n只会从程序里移除这个方案，不会删除系统中已经存在的环境变量。`
    )
  ) {
    return;
  }
  profiles.value = profiles.value.filter((item) => item.id !== profile.id);
  selectedId.value = profiles.value[0]?.id ?? null;
  await persistProfiles();
  setNotice(`已从程序里删除方案“${profile.name}”，系统变量没有改动`);
}

function startCreateVariable() {
  if (!selectedProfile.value) return;
  variableEditor.value = {
    id: null,
    name: "",
    value: "",
    scope: "system",
    expand: true,
    append: true,
  };
  nextTick(() => variableNameInput.value?.focus());
}

function startEditVariable(variable: EnvironmentProfileVariable) {
  variableEditor.value = {
    id: variable.id,
    name: variable.name,
    value: variable.value,
    scope: variable.scope,
    expand: variable.expand,
    append: variable.append,
  };
  nextTick(() => variableNameInput.value?.focus());
}

function cancelVariableEdit() {
  variableEditor.value = null;
}

async function chooseFolder() {
  const editor = variableEditor.value;
  if (!editor) return;
  try {
    const folder = await pickFolder();
    if (folder) {
      editor.value = folder;
    }
  } catch (error) {
    setNotice(String(error), "warn");
  }
}

function isEditing(variable: EnvironmentProfileVariable): boolean {
  return Boolean(variableEditor.value && variableEditor.value.id === variable.id);
}

async function saveVariableEditor() {
  if (!selectedProfile.value || !variableEditor.value || saving.value) return;
  const name = variableEditor.value.name.trim();
  if (!name) {
    setNotice("变量名不能为空", "warn");
    nextTick(() => variableNameInput.value?.focus());
    return;
  }
  if (name.includes("=")) {
    setNotice("变量名不能包含等号（=）", "warn");
    return;
  }

  const duplicate = selectedProfile.value.variables.some(
    (variable) =>
      variable.id !== variableEditor.value?.id &&
      variable.name.trim().toLowerCase() === name.toLowerCase() &&
      variable.scope === variableEditor.value?.scope
  );
  if (duplicate) {
    setNotice(`方案里已经有“${name}”（${variableEditor.value.scope === "system" ? "系统" : "用户"}范围）`, "warn");
    return;
  }

  const variable: EnvironmentProfileVariable = {
    id: variableEditor.value.id ?? makeId("env-variable"),
    name,
    value: variableEditor.value.value,
    scope: variableEditor.value.scope,
    expand: variableEditor.value.expand,
    append: isPathName(name) ? variableEditor.value.append : false,
  };
  const existingIndex = selectedProfile.value.variables.findIndex(
    (item) => item.id === variable.id
  );
  if (existingIndex >= 0) {
    selectedProfile.value.variables[existingIndex] = variable;
  } else {
    selectedProfile.value.variables.push(variable);
  }
  variableEditor.value = null;

  const saved = await persistProfiles();
  if (!saved) return;
  if (!selectedProfile.value.enabled) {
    setNotice(`方案已保存；“${selectedProfile.value.name}”未启用，不会写入系统`);
    return;
  }

  const result = await writeVariable(variable, true);
  await reloadSnapshot();
  if (!isTauri()) {
    setNotice("方案已保存；浏览器预览不会写入注册表");
  } else if (result.ok) {
    const status = getStatus(variable);
    setNotice(
      status.kind === "set"
        ? `“${name}”已经存在于系统中，不需要重复写入`
        : `已把“${name}”写入${variable.scope === "system" ? "系统" : "用户"}环境变量`
    );
  } else {
    setNotice(`方案已保存，但写入系统失败：${result.message ?? "未知错误"}`, "warn");
  }
}

async function writeVariable(
  variable: EnvironmentProfileVariable,
  quiet = false
): Promise<{ ok: boolean; skipped?: boolean; message?: string }> {
  if (!isTauri()) {
    if (!quiet) setNotice("浏览器预览不会修改 Windows 环境变量", "warn");
    return { ok: false, message: "浏览器预览不会修改 Windows 环境变量" };
  }
  const status = getStatus(variable);
  if (status.kind === "set") {
    return { ok: true, skipped: true };
  }

  const existing = findExisting(variable);
  const existingName = existing?.name ?? null;
  const isPath = isPathName(variable.name);
  const normalizedValue = isPath ? normalizePathValue(variable.value) : variable.value;
  const finalValue =
    isPath && variable.append
      ? mergePathValue(existing?.value ?? "", normalizedValue)
      : normalizedValue;

  try {
    await saveEnvironmentVariable(
      variable.scope,
      existingName,
      variable.name.trim(),
      finalValue,
      variable.expand
    );
    if (!quiet) {
      setNotice(`已写入“${variable.name}”（${variable.scope === "system" ? "系统" : "用户"}范围）`);
    }
    return { ok: true };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (!quiet) setNotice(message, "warn");
    return { ok: false, message };
  }
}

async function applySelected() {
  const profile = selectedProfile.value;
  if (!profile || saving.value) return;
  if (!profile.enabled) {
    setNotice(`方案“${profile.name}”未启用，先勾选左侧方案再写入`, "warn");
    return;
  }
  if (!profile.variables.length) {
    setNotice("这个方案还没有变量，先添加一行 key:value", "warn");
    return;
  }
  saving.value = true;
  let written = 0;
  let skipped = 0;
  const failures: string[] = [];
  try {
    for (const variable of profile.variables) {
      const status = getStatus(variable);
      if (status.kind === "set") {
        skipped += 1;
        continue;
      }
      const result = await writeVariable(variable, true);
      if (result.ok) written += 1;
      else failures.push(`${variable.name}：${result.message ?? "写入失败"}`);
    }
    await reloadSnapshot();
    if (!isTauri()) {
      setNotice("浏览器预览不会写入注册表；方案只保存在预览里", "warn");
    } else if (failures.length) {
      setNotice(`已写入 ${written} 项，失败 ${failures.length} 项：${failures[0]}`, "warn");
    } else {
      setNotice(`已写入 ${written} 项，${skipped} 项本来就已设置`);
    }
  } finally {
    saving.value = false;
  }
}

async function applyAllProfiles() {
  if (saving.value) return;
  if (!isTauri()) {
    setNotice("浏览器预览不会写入注册表；方案只保存在预览里", "warn");
    return;
  }
  if (!enabledProfiles.value.length) {
    setNotice("没有已启用的方案，先勾选左侧方案再写入", "warn");
    return;
  }
  if (!enabledVariableCount.value) {
    setNotice("已启用的方案里还没有变量，先添加一行 key:value", "warn");
    return;
  }

  saving.value = true;
  let written = 0;
  let skipped = 0;
  const failures: string[] = [];
  try {
    for (const profile of enabledProfiles.value) {
      for (const variable of profile.variables) {
        const result = await writeVariable(variable, true);
        if (result.ok) {
          if (result.skipped) skipped += 1;
          else written += 1;
        } else {
          failures.push(`${profile.name} / ${variable.name}：${result.message ?? "写入失败"}`);
        }
      }
    }
    await reloadSnapshot();
    if (failures.length) {
      setNotice(`已写入 ${written} 项，失败 ${failures.length} 项：${failures[0]}`, "warn");
    } else if (!written && skipped) {
      setNotice(`全部已设置，共 ${skipped} 项无需重复写入`);
    } else {
      setNotice(
        `已写入 ${written} 项，${skipped} 项本来就已设置；覆盖 ${enabledProfiles.value.length} 个已启用方案`
      );
    }
  } finally {
    saving.value = false;
  }
}

async function deleteSystemVariable(variable: EnvironmentProfileVariable) {
  const existing = findExisting(variable);
  if (!existing) return;
  const pathWarning = isPathName(existing.name)
    ? "\n注意：Path 是整条变量，删除会移除现有的全部路径项，不是只删当前这一段。"
    : "";
  if (
    !window.confirm(
      `从系统中删除环境变量“${existing.name}”（${variable.scope === "system" ? "系统" : "用户"}范围）？\n方案里的这一项会保留，之后还可以重新写入。${pathWarning}`
    )
  ) {
    return;
  }
  try {
    await deleteEnvironmentVariable(variable.scope, existing.name);
    await reloadSnapshot();
    setNotice(`已从系统删除“${existing.name}”，方案中的这一项仍然保留`);
  } catch (error) {
    setNotice(error instanceof Error ? error.message : String(error), "warn");
  }
}

async function removeVariable(variable: EnvironmentProfileVariable) {
  if (!selectedProfile.value) return;
  if (
    !window.confirm(
      `从方案“${selectedProfile.value.name}”里移除“${variable.name}”？\n不会删除系统中的环境变量。`
    )
  ) {
    return;
  }
  selectedProfile.value.variables = selectedProfile.value.variables.filter(
    (item) => item.id !== variable.id
  );
  await persistProfiles();
  setNotice(`已从方案移除“${variable.name}”，系统变量没有改动`);
}

async function toggleProfileEnabled(profile: EnvironmentProfile) {
  const enabled = profile.enabled;
  const saved = await persistProfiles();
  if (!saved) {
    profile.enabled = !enabled;
    return;
  }
  setNotice(
    enabled
      ? `已启用方案“${profile.name}”，可以整组写入`
      : `已停用方案“${profile.name}”，全部写入会跳过整个方案`
  );
}

async function persistProfiles(): Promise<boolean> {
  saving.value = true;
  try {
    const saved = await saveEnvironmentProfiles(profiles.value);
    storePath.value = saved.path;
    return true;
  } catch (error) {
    setNotice(error instanceof Error ? error.message : String(error), "warn");
    return false;
  } finally {
    saving.value = false;
  }
}

async function reloadSnapshot() {
  try {
    snapshot.value = await loadEnvironmentVariables();
  } catch (error) {
    setNotice(error instanceof Error ? error.message : String(error), "warn");
  }
}

async function loadAll(showRefreshNotice = false) {
  loading.value = true;
  loadError.value = "";
  try {
    const [profileStore, environment] = await Promise.all([
      loadEnvironmentProfiles(),
      loadEnvironmentVariables(),
    ]);
    profiles.value = profileStore.profiles;
    storePath.value = profileStore.path;
    snapshot.value = environment;
    if (!profiles.value.some((profile) => profile.id === selectedId.value)) {
      selectedId.value = profiles.value[0]?.id ?? null;
    }
    if (showRefreshNotice) {
      setNotice(`已刷新 ${profiles.value.length} 个方案，并按注册表重新检测`);
    }
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error);
  } finally {
    loading.value = false;
  }
}

function refresh() {
  profileEditor.value = null;
  variableEditor.value = null;
  void loadAll(true);
}

watch(selectedId, () => {
  profileEditor.value = null;
  variableEditor.value = null;
});

onMounted(() => {
  void loadAll();
  window.addEventListener("appshortcut:refresh", handleGlobalRefresh);
});

onBeforeUnmount(() => {
  window.removeEventListener("appshortcut:refresh", handleGlobalRefresh);
});

function handleGlobalRefresh() {
  refresh();
}
</script>

<template>
  <section class="env-page">
    <div class="env-surface panel">
      <header class="env-head">
        <div class="env-head-title">
          <div class="env-head-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="22" height="22">
              <path
                d="M4 7h7M4 12h5M4 17h6M14 6.5a3.5 3.5 0 1 0 7 0 3.5 3.5 0 0 0-7 0ZM15.5 10h4v4.5a4.5 4.5 0 0 1-4 4.47"
                fill="none"
                stroke="currentColor"
                stroke-linecap="round"
              />
            </svg>
          </div>
          <div>
            <h1>环境变量管理</h1>
            <p>按 JAVA / Go 这样的方案维护 key:value，程序启动后按注册表真实状态检测。</p>
          </div>
        </div>
        <div class="env-head-actions">
          <span class="env-count">{{ profiles.length }} 个方案</span>
          <button class="btn btn-ghost env-refresh" type="button" :disabled="loading" @click="refresh">
            <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
              <path
                d="M20 11a8 8 0 0 0-14.6-4.8M4 5v5h5M4 13a8 8 0 0 0 14.6 4.8M20 19v-5h-5"
                fill="none"
                stroke="currentColor"
                stroke-linecap="round"
                stroke-width="1.8"
              />
            </svg>
            刷新检测
          </button>
          <button class="btn btn-primary env-new-profile" type="button" :disabled="loading" @click="startCreateProfile">
            ＋ 新建方案
          </button>
        </div>
      </header>

      <div class="env-body">
        <aside class="env-groups">
          <div class="env-groups-head">
            <span>方案</span>
            <span class="env-groups-count">{{ filteredProfiles.length }} / {{ profiles.length }}</span>
          </div>
          <label class="env-group-search">
            <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
              <path
                d="M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM15.2 15.2 20 20"
                fill="none"
                stroke="currentColor"
                stroke-linecap="round"
                stroke-width="1.7"
              />
            </svg>
            <input v-model="query" type="search" placeholder="搜索方案或变量名" spellcheck="false" />
          </label>
          <div class="env-group-list scroll-area">
            <div v-if="profileEditor" class="env-group-editor">
              <input
                ref="profileNameInput"
                v-model="profileEditor.name"
                type="text"
                placeholder="方案名，如 JAVA"
                spellcheck="false"
                @keydown.enter.prevent="saveProfileEditor"
                @keydown.esc.prevent="profileEditor = null"
              />
              <div class="env-group-editor-actions">
                <button class="env-mini-btn primary" type="button" @click="saveProfileEditor">保存</button>
                <button class="env-mini-btn" type="button" @click="profileEditor = null">取消</button>
              </div>
            </div>

            <div
              v-for="profile in filteredProfiles"
              :key="profile.id"
              class="env-group-item"
              :class="{ active: profile.id === selectedId, 'is-off': !profile.enabled }"
              role="button"
              tabindex="0"
              @click="selectProfile(profile)"
              @keydown.enter.prevent="selectProfile(profile)"
              @keydown.space.prevent="selectProfile(profile)"
            >
              <label class="env-group-check" :title="profile.enabled ? '整个方案参与写入' : '整个方案不写入'">
                <input
                  v-model="profile.enabled"
                  type="checkbox"
                  @change="toggleProfileEnabled(profile)"
                  @click.stop
                />
              </label>
              <span class="env-group-name">{{ profile.name }}</span>
              <span class="env-group-meta">{{ profile.variables.length }} 项</span>
              <span class="env-group-status" :class="`is-${profileSummaryTone(profile)}`">
                {{ profileSummary(profile) }}
              </span>
            </div>

            <div v-if="!filteredProfiles.length" class="env-group-empty">
              {{ profiles.length ? "没有匹配的方案" : "还没有方案，点下方新建" }}
            </div>
          </div>
          <div class="env-group-actions">
            <button
              class="btn btn-primary env-group-write-all"
              type="button"
              :disabled="saving || loading || !enabledProfiles.length || !enabledVariableCount"
              title="写入所有已启用方案"
              @click="applyAllProfiles"
            >
              全部写入
            </button>
            <button class="env-group-add" type="button" @click="startCreateProfile">＋ 新建方案</button>
          </div>
        </aside>

        <main class="env-main">
          <div v-if="loadError" class="env-readonly">
            <span class="env-readonly-icon">!</span>
            <span>{{ loadError }}</span>
          </div>

          <div v-if="!selectedProfile" class="env-empty-state">
            <div class="env-empty-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="26" height="26">
                <path
                  d="M4 7h7M4 12h5M4 17h6M14 6.5a3.5 3.5 0 1 0 7 0 3.5 3.5 0 0 0-7 0ZM15.5 10h4v4.5a4.5 4.5 0 0 1-4 4.47"
                  fill="none"
                  stroke="currentColor"
                  stroke-linecap="round"
                />
              </svg>
            </div>
            <h2>先新建一个环境变量方案</h2>
            <p>比如 JAVA：把 JAVA_HOME、CLASSPATH、Path 作为几行 key:value 加进来。方案只保存你要设置的值，是否已经存在由注册表实时检测。</p>
            <button class="btn btn-primary" type="button" @click="startCreateProfile">＋ 新建方案</button>
          </div>

          <template v-else>
            <div class="env-profile-head">
              <div class="env-profile-title">
                <div class="env-profile-title-line">
                  <h2>{{ selectedProfile.name }}</h2>
                  <button class="env-icon-btn" type="button" title="修改方案名称" @click="startRenameProfile(selectedProfile)">
                    <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
                      <path
                        d="m4 20 4.2-1 10.9-10.9a1.8 1.8 0 0 0 0-2.5l-1.7-1.7a1.8 1.8 0 0 0-2.5 0L4 14.8 3 19l1 1Z"
                        fill="none"
                        stroke="currentColor"
                        stroke-linecap="round"
                        stroke-width="1.6"
                      />
                    </svg>
                  </button>
                  <span class="env-profile-tag">新变量默认系统</span>
                  <span v-if="!selectedProfile.enabled" class="env-profile-tag off">方案未启用</span>
                </div>
                <p>方案只保存 key:value 定义；已设置 / 缺失以注册表实际值为准，不保存“已创建”标记。</p>
              </div>
              <div class="env-profile-actions">
                <button
                  class="btn btn-primary env-apply-all"
                  type="button"
                  :disabled="saving || loading || !selectedProfile.enabled || !selectedProfile.variables.length"
                  title="只写入当前方案"
                  @click="applySelected"
                >
                  写入方案
                </button>
                <button class="btn btn-ghost env-delete-profile" type="button" @click="deleteProfile">
                  删除方案
                </button>
              </div>
            </div>

            <div v-if="isBrowserPreview" class="env-readonly preview">
              <span class="env-readonly-icon">i</span>
              <span>浏览器预览：方案只是示例，不会读取或修改真实注册表。</span>
            </div>
            <div v-else-if="snapshot && !systemWritable" class="env-readonly">
              <span class="env-readonly-icon">!</span>
              <span>当前不是管理员：系统范围变量暂时写不进去。方案仍会保存，右键 WinKit.exe「以管理员身份运行」后点左侧“全部写入”即可。</span>
            </div>

            <div class="env-toolbar">
              <div class="env-toolbar-summary">
                <span>{{ selectedCounts.total }} 项变量</span>
                <span v-if="!selectedProfile.enabled" class="env-toolbar-off">· 方案未启用</span>
                <template v-else>
                  <span class="env-toolbar-divider">·</span>
                  <span>已设置 {{ selectedCounts.setCount }}</span>
                  <span v-if="selectedCounts.pendingCount" class="env-toolbar-pending">
                    待处理 {{ selectedCounts.pendingCount }}
                  </span>
                  <span v-if="selectedCounts.errorCount" class="env-toolbar-error">
                    {{ selectedCounts.errorCount }} 项读取失败
                  </span>
                </template>
              </div>
              <button class="btn btn-ghost env-add-variable" type="button" :disabled="saving" @click="startCreateVariable">
                ＋ 添加变量
              </button>
            </div>

            <div class="env-table">
              <div class="env-table-head">
                <span>变量名</span>
                <span>值</span>
                <span>范围</span>
                <span>状态</span>
                <span class="env-table-actions-head">操作</span>
              </div>
              <div class="env-list scroll-area">
                <div
                  v-for="variable in selectedProfile.variables"
                  :key="variable.id"
                  class="env-row"
                  :class="{ editing: isEditing(variable), 'is-off': !selectedProfile.enabled }"
                >
                  <template v-if="variableEditor && variableEditor.id === variable.id">
                    <input
                      :ref="setVariableNameInput"
                      v-model="variableEditor.name"
                      class="env-input env-name-input"
                      type="text"
                      placeholder="变量名，如 JAVA_HOME"
                      spellcheck="false"
                      @keydown.esc.prevent="cancelVariableEdit"
                    />
                    <div class="env-value-edit">
                      <input
                        v-model="variableEditor.value"
                        class="env-input env-value-input"
                        type="text"
                        placeholder="值，如 D:\application\jdk-25"
                        spellcheck="false"
                        @keydown.esc.prevent="cancelVariableEdit"
                      />
                      <button class="env-folder-btn" type="button" title="选择文件夹" @click="chooseFolder">
                        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                          <path
                            d="M3.5 7.5a2 2 0 0 1 2-2h3.2l2 2h7.8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z"
                            fill="none"
                            stroke="currentColor"
                            stroke-linejoin="round"
                            stroke-width="1.6"
                          />
                        </svg>
                      </button>
                    </div>
                    <select v-model="variableEditor.scope" class="env-select">
                      <option value="system">系统</option>
                      <option value="user">用户</option>
                    </select>
                    <div class="env-edit-toggles">
                      <label class="env-type-toggle" title="勾选后写入 REG_EXPAND_SZ，值里的 %变量% 会展开">
                        <input v-model="variableEditor.expand" type="checkbox" />
                        <span>展开</span>
                      </label>
                      <label v-if="isPathEditor" class="env-type-toggle" title="追加到现有 Path 末尾，不覆盖已有项">
                        <input v-model="variableEditor.append" type="checkbox" />
                        <span>追加</span>
                      </label>
                    </div>
                    <div class="env-row-actions">
                      <button class="env-save-btn" type="button" :disabled="saving" @click="saveVariableEditor">保存</button>
                      <button class="env-cancel-btn" type="button" @click="cancelVariableEdit">取消</button>
                    </div>
                  </template>

                  <template v-else>
                    <span class="env-name" :title="variable.name">{{ variable.name }}</span>
                    <div class="env-value-wrap">
                      <span class="env-value" :title="variable.value || '空值'">
                        {{ variable.value || "（空值）" }}
                      </span>
                      <span v-if="getStatus(variable).current && getStatus(variable).kind !== 'set'" class="env-current" :title="getStatus(variable).current">
                        当前：{{ getStatus(variable).current }}
                      </span>
                    </div>
                    <span class="env-scope-chip" :class="variable.scope">
                      {{ variable.scope === "system" ? "系统" : "用户" }}
                    </span>
                    <span class="env-status" :class="`is-${getStatus(variable).kind}`">
                      {{ getStatus(variable).label }}
                    </span>
                    <div class="env-row-actions">
                      <button
                        v-if="selectedProfile.enabled && getStatus(variable).kind !== 'set' && getStatus(variable).kind !== 'loading'"
                        class="env-write-btn"
                        type="button"
                        :disabled="saving"
                        :title="getStatus(variable).kind === 'missing' ? '写入系统' : '更新系统值'"
                        @click="writeVariable(variable)"
                      >
                        {{ getStatus(variable).kind === "missing" ? "写入" : "更新" }}
                      </button>
                      <button
                        v-if="getStatus(variable).existingName"
                        class="env-icon-btn danger"
                        type="button"
                        title="删除系统变量（方案项保留）"
                        @click="deleteSystemVariable(variable)"
                      >
                        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                          <path
                            d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v5M14 11v5"
                            fill="none"
                            stroke="currentColor"
                            stroke-linecap="round"
                            stroke-width="1.6"
                          />
                        </svg>
                      </button>
                      <button class="env-icon-btn" type="button" title="编辑方案项" @click="startEditVariable(variable)">
                        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                          <path
                            d="m4 20 4.2-1 10.9-10.9a1.8 1.8 0 0 0 0-2.5l-1.7-1.7a1.8 1.8 0 0 0-2.5 0L4 14.8 3 19l1 1Z"
                            fill="none"
                            stroke="currentColor"
                            stroke-linecap="round"
                            stroke-width="1.6"
                          />
                        </svg>
                      </button>
                      <button
                        class="env-icon-btn danger"
                        type="button"
                        title="从方案移除（不动系统变量）"
                        @click="removeVariable(variable)"
                      >
                        ×
                      </button>
                    </div>
                  </template>
                </div>

                <div v-if="variableEditor && !variableEditor.id" class="env-row editing">
                  <input
                    :ref="setVariableNameInput"
                    v-model="variableEditor.name"
                    class="env-input env-name-input"
                    type="text"
                    placeholder="变量名，如 JAVA_HOME"
                    spellcheck="false"
                    @keydown.esc.prevent="cancelVariableEdit"
                  />
                  <div class="env-value-edit">
                    <input
                      v-model="variableEditor.value"
                      class="env-input env-value-input"
                      type="text"
                      placeholder="值，如 D:\application\jdk-25"
                      spellcheck="false"
                      @keydown.esc.prevent="cancelVariableEdit"
                    />
                    <button class="env-folder-btn" type="button" title="选择文件夹" @click="chooseFolder">
                      <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                        <path
                          d="M3.5 7.5a2 2 0 0 1 2-2h3.2l2 2h7.8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z"
                          fill="none"
                          stroke="currentColor"
                          stroke-linejoin="round"
                          stroke-width="1.6"
                        />
                      </svg>
                    </button>
                  </div>
                  <select v-model="variableEditor.scope" class="env-select">
                    <option value="system">系统</option>
                    <option value="user">用户</option>
                  </select>
                  <div class="env-edit-toggles">
                    <label class="env-type-toggle" title="勾选后写入 REG_EXPAND_SZ，值里的 %变量% 会展开">
                      <input v-model="variableEditor.expand" type="checkbox" />
                      <span>展开</span>
                    </label>
                    <label v-if="isPathEditor" class="env-type-toggle" title="追加到现有 Path 末尾，不覆盖已有项">
                      <input v-model="variableEditor.append" type="checkbox" />
                      <span>追加</span>
                    </label>
                  </div>
                  <div class="env-row-actions">
                    <button class="env-save-btn" type="button" :disabled="saving" @click="saveVariableEditor">保存</button>
                    <button class="env-cancel-btn" type="button" @click="cancelVariableEdit">取消</button>
                  </div>
                </div>

                <div v-if="!selectedProfile.variables.length && !variableEditor" class="env-empty">
                  <h3>这个方案还没有变量</h3>
                  <p>添加一行 key:value，比如 `JAVA_HOME = D:\application\jdk-25`。保存后会尝试写入系统，缺失时状态仍会如实显示。</p>
                  <button class="btn btn-primary" type="button" @click="startCreateVariable">＋ 添加变量</button>
                </div>
              </div>
            </div>

            <footer class="env-footer">
              <span class="env-footer-path" :title="storePath">
                方案文件：{{ storePath || "读取中…" }}
              </span>
              <span class="env-footer-tip">
                点铅笔改方案项，垃圾桶只删系统变量，× 只从方案移除；F5 / Ctrl+R 重新检测。
              </span>
            </footer>
          </template>
        </main>
      </div>
    </div>
  </section>
</template>

<style scoped>
.env-page {
  flex: 1;
  min-height: 0;
  display: flex;
  padding: 0 18px 18px;
}

.env-surface {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: var(--radius-lg);
}

.env-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 18px;
  padding: 16px 18px 14px;
  border-bottom: 1px solid var(--line);
  background: linear-gradient(180deg, #fbfdff 0%, #f7fafe 100%);
}

.env-head-title {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  min-width: 0;
}

.env-head-icon {
  width: 42px;
  height: 42px;
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  border-radius: 12px;
  color: var(--accent);
  background: #eaf2fb;
}

.env-head h1 {
  margin: 0;
  font-size: 19px;
  line-height: 1.35;
  letter-spacing: 0.01em;
}

.env-head p {
  margin: 5px 0 0;
  color: var(--text-muted);
  font-size: 12.5px;
  line-height: 1.55;
}

.env-head-actions {
  display: flex;
  align-items: center;
  gap: 9px;
  flex: 0 0 auto;
}

.env-count {
  color: var(--text-muted);
  font-size: 12px;
  white-space: nowrap;
  margin-right: 2px;
}

.env-refresh svg {
  margin-right: 4px;
}

.env-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 232px minmax(0, 1fr);
}

.env-groups {
  min-height: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--line);
  background: #fbfcfe;
}

.env-groups-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 15px 14px 9px;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.08em;
  color: var(--text-muted);
}

.env-groups-count {
  font-weight: 500;
  letter-spacing: 0;
  color: #8a99aa;
}

.env-group-search {
  margin: 0 12px 10px;
  height: 34px;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 0 10px;
  border: 1px solid var(--line);
  border-radius: 9px;
  color: #8b99aa;
  background: #fff;
}

.env-group-search:focus-within {
  border-color: #8dbbeb;
  box-shadow: 0 0 0 3px rgba(15, 108, 189, 0.09);
}

.env-group-search input {
  width: 100%;
  min-width: 0;
  border: 0;
  outline: 0;
  background: transparent;
  font: inherit;
  font-size: 12.5px;
  color: var(--text);
}

.env-group-list {
  flex: 1;
  min-height: 0;
  padding: 0 8px 8px;
}

.env-group-editor {
  margin: 2px 4px 8px;
  padding: 8px;
  border: 1px solid #9cc4e8;
  border-radius: 9px;
  background: #fff;
}

.env-group-editor input {
  width: 100%;
  height: 30px;
  box-sizing: border-box;
  border: 1px solid #b8d5ee;
  border-radius: 7px;
  padding: 0 8px;
  font: inherit;
  font-size: 12.5px;
  outline: 0;
}

.env-group-editor-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
  margin-top: 7px;
}

.env-mini-btn {
  border: 1px solid var(--line-strong);
  border-radius: 7px;
  background: #fff;
  color: var(--text);
  padding: 4px 9px;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.env-mini-btn.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #fff;
}

.env-group-item {
  width: 100%;
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr) auto;
  grid-template-rows: auto auto;
  gap: 3px 8px;
  padding: 10px 10px 9px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: transparent;
  text-align: left;
  cursor: pointer;
  color: var(--text);
  transition: background 0.15s ease, border-color 0.15s ease;
}

.env-group-item:hover {
  background: #f2f7fc;
}

.env-group-item.active {
  border-color: #b7d7f2;
  background: #eaf3fc;
}

.env-group-item.is-off .env-group-name {
  color: #8b99aa;
}

.env-group-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  font-weight: 700;
}

.env-group-meta {
  color: var(--text-muted);
  font-size: 11px;
  white-space: nowrap;
}

.env-group-status {
  grid-column: 2 / -1;
  width: fit-content;
  max-width: 100%;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: 10.5px;
  line-height: 1.5;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.env-group-status.is-done {
  color: #1f6b3b;
  background: #e7f5eb;
}

.env-group-status.is-pending {
  color: #8a5a08;
  background: #fff3db;
}

.env-group-status.is-error {
  color: #9f2f24;
  background: #fdebea;
}

.env-group-status.is-empty,
.env-group-status.is-loading {
  color: #687789;
  background: #eff3f8;
}

.env-group-empty {
  padding: 18px 10px;
  color: #8b99aa;
  font-size: 12px;
  line-height: 1.7;
  text-align: center;
}

.env-group-add {
  margin: 0 12px 12px;
  height: 34px;
  border: 1px dashed #aec6dd;
  border-radius: 9px;
  background: #fff;
  color: var(--accent);
  font: inherit;
  font-size: 12.5px;
  font-weight: 600;
  cursor: pointer;
}

.env-group-add:hover {
  border-color: #7eace0;
  background: #f4f9fe;
}

.env-group-actions {
  margin: 0 12px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.env-group-actions .env-group-add {
  margin: 0;
}

.env-group-write-all {
  width: 100%;
  height: 36px;
  padding: 0 12px;
}

.env-main {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 14px 16px 12px;
  background: #fff;
}

.env-readonly {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-bottom: 10px;
  padding: 9px 11px;
  border: 1px solid #e7c98e;
  border-radius: 9px;
  color: #7a5208;
  background: #fff8e8;
  font-size: 12px;
  line-height: 1.55;
}

.env-readonly.preview {
  border-color: #bfd5ea;
  color: #315a80;
  background: #f1f7fd;
}

.env-readonly-icon {
  flex: 0 0 auto;
  width: 17px;
  height: 17px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: rgba(122, 82, 8, 0.14);
  font-size: 11px;
  font-weight: 700;
}

.env-readonly.preview .env-readonly-icon {
  background: rgba(49, 90, 128, 0.14);
}

.env-profile-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 10px;
}

.env-profile-title {
  min-width: 0;
}

.env-profile-title-line {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
}

.env-profile-title-line h2 {
  margin: 0;
  font-size: 18px;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.env-profile-tag {
  flex: 0 0 auto;
  padding: 2px 7px;
  border-radius: 999px;
  color: #526274;
  background: #eff3f8;
  font-size: 10.5px;
}

.env-profile-tag.off {
  color: #8a5a08;
  background: #fff3db;
}

.env-profile-title p {
  margin: 4px 0 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.55;
}

.env-profile-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 0 0 auto;
}

.env-profile-editor {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-bottom: 10px;
  padding: 8px 9px;
  border: 1px solid #b8d5ee;
  border-radius: 9px;
  background: #f7fbff;
}

.env-profile-editor input {
  flex: 1;
  min-width: 0;
  height: 32px;
  box-sizing: border-box;
  border: 1px solid #b8d5ee;
  border-radius: 7px;
  padding: 0 9px;
  font: inherit;
  font-size: 13px;
  outline: 0;
}

.env-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 0 7px;
  border-top: 1px solid #edf1f6;
}

.env-toolbar-summary {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-muted);
  font-size: 12px;
  white-space: nowrap;
}

.env-toolbar-divider {
  color: #c0cad6;
}

.env-toolbar-pending {
  color: #8a5a08;
}

.env-toolbar-off {
  color: #8b99aa;
}

.env-toolbar-error {
  color: #9f2f24;
}

.env-add-variable {
  height: 32px !important;
  padding: 0 11px !important;
  font-size: 12.5px !important;
}

.env-table {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid #e4ebf3;
  border-radius: 10px;
  overflow: hidden;
}

.env-table-head,
.env-row {
  display: grid;
  grid-template-columns: minmax(154px, 0.95fr) minmax(0, 1.5fr) 70px 96px 140px;
  align-items: center;
  gap: 8px;
}

.env-table-head {
  flex: 0 0 auto;
  padding: 8px 11px;
  background: #f5f8fc;
  border-bottom: 1px solid #e4ebf3;
  color: #687789;
  font-size: 11.5px;
  font-weight: 600;
}

.env-table-actions-head {
  text-align: right;
}

.env-group-check {
  grid-row: 1 / span 2;
  align-self: center;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.env-group-check input {
  width: 15px;
  height: 15px;
  margin: 0;
  accent-color: var(--accent);
  cursor: pointer;
}

.env-list {
  flex: 1;
  min-height: 0;
}

.env-row {
  min-height: 48px;
  padding: 7px 11px;
  border-bottom: 1px solid #edf1f6;
  font-size: 12.5px;
}

.env-row:last-child {
  border-bottom: 0;
}

.env-row.editing {
  min-height: 46px;
  padding-top: 6px;
  padding-bottom: 6px;
  background: #f8fbfe;
}

.env-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: "Cascadia Mono", Consolas, monospace;
  font-size: 12px;
  font-weight: 600;
}

.env-value-wrap {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.env-value {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #26374a;
  font-family: "Cascadia Mono", Consolas, monospace;
  font-size: 11.5px;
}

.env-current {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #8b99aa;
  font-family: "Cascadia Mono", Consolas, monospace;
  font-size: 10.5px;
}

.env-scope-chip {
  width: fit-content;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: 10.5px;
  background: #eff3f8;
  color: #526274;
}

.env-scope-chip.system {
  color: #315a80;
  background: #eaf2fb;
}

.env-scope-chip.user {
  color: #426254;
  background: #eef6f1;
}

.env-status {
  width: fit-content;
  max-width: 100%;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: 10.5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.env-status.is-set {
  color: #1f6b3b;
  background: #e7f5eb;
}

.env-status.is-missing {
  color: #8a5a08;
  background: #fff3db;
}

.env-status.is-diff,
.env-status.is-partial {
  color: #2f5b80;
  background: #eaf2fb;
}

.env-status.is-error {
  color: #9f2f24;
  background: #fdebea;
}

.env-status.is-loading,
.env-status.is-disabled {
  color: #687789;
  background: #eff3f8;
}

.env-row.is-off .env-name,
.env-row.is-off .env-value {
  color: #8b99aa;
}

.env-row-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 3px;
}

.env-write-btn {
  height: 26px;
  padding: 0 8px;
  border: 1px solid #b8d5ee;
  border-radius: 7px;
  background: #f4f9fe;
  color: var(--accent);
  font: inherit;
  font-size: 11.5px;
  font-weight: 600;
  cursor: pointer;
}

.env-write-btn:hover {
  background: #e8f3fd;
}

.env-write-btn:disabled {
  opacity: 0.55;
  cursor: default;
}

.env-icon-btn {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: #7d8b9b;
  font: inherit;
  font-size: 15px;
  line-height: 1;
  cursor: pointer;
}

.env-icon-btn:hover {
  color: var(--accent);
  background: #eff5fb;
}

.env-icon-btn.danger:hover {
  color: var(--danger);
  background: #fdf0ef;
}

.env-input {
  width: 100%;
  min-width: 0;
  height: 32px;
  box-sizing: border-box;
  border: 1px solid #b8d5ee;
  border-radius: 7px;
  padding: 0 8px;
  font: inherit;
  font-size: 12px;
  color: var(--text);
  outline: 0;
  background: #fff;
}

.env-input:focus {
  border-color: #7eace0;
  box-shadow: 0 0 0 3px rgba(15, 108, 189, 0.08);
}

.env-name-input {
  font-family: "Cascadia Mono", Consolas, monospace;
  font-weight: 600;
}

.env-value-input {
  font-family: "Cascadia Mono", Consolas, monospace;
}

.env-value-edit {
  position: relative;
  min-width: 0;
}

.env-value-edit .env-value-input {
  padding-right: 30px;
}

.env-folder-btn {
  position: absolute;
  top: 50%;
  right: 3px;
  transform: translateY(-50%);
  display: inline-grid;
  place-items: center;
  width: 25px;
  height: 25px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: #55708c;
  cursor: pointer;
}

.env-folder-btn:hover {
  background: #e8f1fa;
  color: var(--accent);
}

.env-select {
  width: 100%;
  height: 32px;
  box-sizing: border-box;
  border: 1px solid #cbd6e3;
  border-radius: 7px;
  padding: 0 5px;
  background: #fff;
  color: var(--text);
  font: inherit;
  font-size: 11.5px;
  outline: 0;
}

.env-edit-toggles {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-wrap: wrap;
}

.env-type-toggle {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: #526274;
  font-size: 10.5px;
  white-space: nowrap;
  cursor: pointer;
}

.env-type-toggle input {
  margin: 0;
  accent-color: var(--accent);
}

.env-save-btn,
.env-cancel-btn {
  height: 26px;
  padding: 0 8px;
  border-radius: 7px;
  font: inherit;
  font-size: 11.5px;
  font-weight: 600;
  cursor: pointer;
}

.env-save-btn {
  border: 1px solid var(--accent);
  background: var(--accent);
  color: #fff;
}

.env-save-btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.env-cancel-btn {
  border: 1px solid var(--line-strong);
  background: #fff;
  color: #526274;
}

.env-empty,
.env-empty-state {
  text-align: center;
  color: var(--text-muted);
}

.env-empty {
  padding: 46px 20px 34px;
}

.env-empty h3 {
  margin: 0 0 7px;
  color: var(--text);
  font-size: 14px;
}

.env-empty p {
  max-width: 460px;
  margin: 0 auto 14px;
  font-size: 12px;
  line-height: 1.7;
}

.env-empty-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px;
}

.env-empty-icon {
  width: 54px;
  height: 54px;
  display: grid;
  place-items: center;
  border-radius: 15px;
  color: var(--accent);
  background: #eaf2fb;
}

.env-empty-state h2 {
  margin: 14px 0 6px;
  font-size: 16px;
  color: var(--text);
}

.env-empty-state p {
  max-width: 460px;
  margin: 0 0 16px;
  font-size: 12.5px;
  line-height: 1.75;
}

.env-footer {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 1px 0;
  color: #8796a7;
  font-size: 11px;
}

.env-footer-path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.env-footer-tip {
  flex: 0 0 auto;
  color: #8b99aa;
  white-space: nowrap;
}
</style>

