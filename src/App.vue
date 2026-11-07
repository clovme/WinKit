<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import AppHeader from "./components/AppHeader.vue";
import ProgramToolbar from "./components/ProgramToolbar.vue";
import ProgramRow from "./components/ProgramRow.vue";
import PlanPanel from "./components/PlanPanel.vue";
import ProcessManager from "./components/ProcessManager.vue";
import EnvironmentManager from "./components/EnvironmentManager.vue";
import ServiceManager from "./components/ServiceManager.vue";
import ToastStack from "./components/ToastStack.vue";
import { useShortcutLibrary } from "./composables/useShortcutLibrary";
import { isTauri, pickPrograms } from "./services/shortcutService";

const {
  programs,
  options,
  search,
  creating,
  refreshing,
  progress,
  toasts,
  storagePath,
  storagePortable,
  storageReady,
  filteredPrograms,
  includedCount,
  excludedCount,
  groupOptions,
  plannedCount,
  remainingCount,
  notify,
  refreshNow,
  setAllIncluded,
  addPrograms,
  removeProgram,
  setIncluded,
  setScope,
  setTargets,
  setGroup,
  renameProgram,
  toggleOption,
  clearCreatedMarks,
  createOne,
  createAll,
  iconFor,
} = useShortcutLibrary();

const storageLabel = computed(() => {
  if (!storageReady.value) return "清单加载中…";
  if (!isTauri()) return "浏览器预览 · 清单存于浏览器";
  return storagePortable.value ? "清单已保存到程序目录" : "清单已保存到 AppData";
});

const allIncluded = computed(
  () => programs.value.length > 0 && includedCount.value === programs.value.length
);
const someIncluded = computed(() => includedCount.value > 0 && !allIncluded.value);
const picking = ref(false);
type AppView = "processes" | "environment" | "services" | "shortcuts";
const view = ref<AppView>("processes");

function toggleAll() {
  setAllIncluded(!allIncluded.value);
}

async function handleAdd() {
  if (picking.value) return;
  picking.value = true;
  try {
    const picked = await pickPrograms();
    if (picked.length) addPrograms(picked);
  } catch (error) {
    notify(error instanceof Error ? error.message : "打开系统文件选择窗口失败", "danger");
  } finally {
    picking.value = false;
  }
}

function handleRefreshShortcut() {
  if (view.value === "shortcuts") void refreshNow();
}

onMounted(() => {
  window.addEventListener("appshortcut:refresh", handleRefreshShortcut);
});
onBeforeUnmount(() => {
  window.removeEventListener("appshortcut:refresh", handleRefreshShortcut);
});
</script>

<template>
  <div class="app-shell">
    <AppHeader
      :included="includedCount"
      :total="programs.length"
      :view="view"
      @switch="view = $event"
    />

    <div v-if="view === 'processes'" class="app-body process-body">
      <ProcessManager embedded />
    </div>

    <div v-else-if="view === 'environment'" class="app-body process-body">
      <EnvironmentManager />
    </div>

    <div v-else-if="view === 'services'" class="app-body process-body">
      <ServiceManager />
    </div>

    <div v-else class="app-body">
      <main class="workspace">
        <section class="panel list-panel">
          <ProgramToolbar
            :total="programs.length"
            :included="includedCount"
            :search="search"
            :picking="picking"
            :refreshing="refreshing"
            @update:search="search = $event"
            @add="handleAdd"
            @refresh="refreshNow"
          />

          <div class="rows scroll-area">
            <div class="table-head">
              <label class="check check-all" title="全选 / 全不选">
                <input type="checkbox" :checked="allIncluded" @change="toggleAll" />
                <span class="check-box" :class="{ partial: someIncluded }">
                  <svg v-if="someIncluded" viewBox="0 0 12 12" width="11" height="11" aria-hidden="true">
                    <path d="M2.5 6h7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
                  </svg>
                  <svg v-else viewBox="0 0 12 12" width="11" height="11" aria-hidden="true">
                    <path d="M2 6.2 4.7 9 10 3.2" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round" stroke-linejoin="round" />
                  </svg>
                </span>
              </label>
              <span>程序</span>
              <span>创建位置 / 分组</span>
              <span>范围</span>
              <span>状态</span>
              <span class="head-actions">操作</span>
            </div>

            <ProgramRow
              v-for="program in filteredPrograms"
              :key="program.id"
              :item="program"
              :group-options="groupOptions"
              :icon="iconFor(program.path)"
              @toggle="setIncluded(program.id, $event)"
              @scope="setScope(program.id, $event)"
              @targets="setTargets(program.id, $event)"
              @rename="renameProgram(program.id, $event)"
              @group="setGroup(program.id, $event)"
              @create="createOne(program.id)"
              @remove="removeProgram(program.id)"
            />

            <div v-if="!filteredPrograms.length" class="empty">
              <div class="empty-mark">
                <svg viewBox="0 0 32 32" width="32" height="32" aria-hidden="true">
                  <rect x="4" y="6" width="24" height="20" rx="4" fill="none" stroke="currentColor" stroke-width="1.8" />
                  <path d="M10 14h12M10 19h7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
                </svg>
              </div>
              <h3>{{ programs.length ? "没有匹配的程序" : "清单还是空的" }}</h3>
              <p>
                {{
                  programs.length
                    ? "换个关键词搜索，或清空搜索查看全部程序"
                    : "点击“添加程序”，在弹出的系统窗口中选择需要创建快捷方式的 .exe"
                }}
              </p>
              <button
                v-if="!programs.length"
                class="btn btn-primary"
                type="button"
                :disabled="picking"
                @click="handleAdd"
              >
                {{ picking ? "选择中…" : "添加第一个程序" }}
              </button>
              <button v-else class="btn" type="button" @click="search = ''">清空搜索</button>
            </div>
          </div>

          <div class="list-footer">
            <div class="footer-left">
              <span>共 {{ programs.length }} 个程序 · 已排除 {{ excludedCount }} 个</span>
              <span class="footer-sep">·</span>
              <span class="footer-storage" :title="storagePath || '清单文件路径'">
                {{ storageLabel }}
              </span>
            </div>
            <span class="footer-hint">取消勾选即排除该程序；每行可单独切换桌面 / 开始菜单</span>
          </div>
        </section>
      </main>

      <PlanPanel
        :options="options"
        :planned="plannedCount"
        :remaining="remainingCount"
        :creating="creating"
        :progress-done="progress.done"
        :progress-total="progress.total"
        @toggle-option="toggleOption"
        @create-all="createAll"
        @clear-marks="clearCreatedMarks"
      />
    </div>

    <ToastStack :toasts="toasts" />
  </div>
</template>

<style scoped>
.app-shell {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: radial-gradient(900px 380px at 8% -12%, rgba(15, 108, 189, 0.1), transparent 60%),
    radial-gradient(720px 320px at 100% 0%, rgba(14, 116, 144, 0.06), transparent 65%),
    linear-gradient(180deg, #f4f8fc 0%, var(--bg) 100%);
  overflow: hidden;
}

.app-body {
  flex: 1;
  display: flex;
  gap: 16px;
  padding: 0 22px 20px;
  min-height: 0;
}

.process-body {
  align-items: stretch;
}

.workspace {
  flex: 1;
  min-width: 0;
  display: flex;
}

.list-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.rows {
  flex: 1;
  min-height: 0;
  position: relative;
  padding-bottom: 6px;
}

.table-head {
  position: sticky;
  top: 0;
  z-index: 2;
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) 132px 86px 86px 104px;
  align-items: center;
  gap: 12px;
  padding: 8px 18px 10px;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-3);
  background: #ffffff;
  border-bottom: 1px solid var(--line);
}

.head-actions {
  text-align: right;
}

.check-box.partial {
  background: var(--accent);
  border-color: var(--accent);
  color: #ffffff;
}

.check-box.partial svg {
  opacity: 1;
  transform: none;
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 56px 24px;
  text-align: center;
}

.empty-mark {
  width: 60px;
  height: 60px;
  border-radius: 16px;
  display: grid;
  place-items: center;
  color: var(--accent);
  background: var(--accent-soft);
  margin-bottom: 4px;
}

.empty h3 {
  font-size: 15px;
  font-weight: 700;
  color: var(--text);
}

.empty p {
  max-width: 320px;
  font-size: 12.5px;
  color: var(--text-2);
  line-height: 1.6;
}

.empty .btn {
  margin-top: 6px;
}

.list-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 10px 18px 14px;
  border-top: 1px solid var(--line);
  font-size: 11.5px;
  color: var(--text-3);
  flex: none;
}

.footer-left {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
}

.footer-sep {
  color: var(--line-strong);
}

.footer-storage {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: default;
}

.footer-hint {
  color: var(--text-3);
  white-space: nowrap;
}

@media (max-width: 1040px) {
  .app-body {
    gap: 12px;
    padding: 0 16px 16px;
  }
}
</style>
