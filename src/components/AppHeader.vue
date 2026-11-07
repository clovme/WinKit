<script setup lang="ts">
type AppView = "processes" | "environment" | "services" | "shortcuts";
defineProps<{ included: number; total: number; view: AppView }>();
const emit = defineEmits<{ switch: [view: AppView] }>();
</script>

<template>
  <header class="app-header">
    <div class="brand">
      <div class="brand-mark">
        <svg viewBox="0 0 34 34" width="34" height="34" aria-hidden="true">
          <defs>
            <linearGradient id="brandGradient" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stop-color="#2f8fe0" />
              <stop offset="1" stop-color="#0a579b" />
            </linearGradient>
          </defs>
          <rect x="1.5" y="1.5" width="27" height="27" rx="8.5" fill="url(#brandGradient)" />
          <rect x="7" y="8" width="16" height="3" rx="1.5" fill="#ffffff" opacity="0.92" />
          <rect x="7" y="13.5" width="11" height="3" rx="1.5" fill="#ffffff" opacity="0.62" />
          <rect x="7" y="19" width="13.5" height="3" rx="1.5" fill="#ffffff" opacity="0.62" />
          <circle cx="25.5" cy="25.5" r="7.5" fill="#ffffff" stroke="#e2e8f0" stroke-width="1" />
          <path
            d="M21.9 25.5h7.2m0 0-2.5-2.5m2.5 2.5-2.5 2.5"
            stroke="#0f6cbd"
            stroke-width="1.9"
            stroke-linecap="round"
            stroke-linejoin="round"
            fill="none"
          />
        </svg>
      </div>
      <div class="brand-copy">
        <h1>
          WinKit 工具箱
          <span class="brand-en">TOOLKIT</span>
        </h1>
        <p>
          {{
            view === "processes"
              ? "WebView2、node 等多实例进程，筛选后批量结束"
              : view === "environment"
                ? "按 JAVA / Go 方案维护，启动后真实检测"
                : view === "services"
                  ? "把任意 exe 包装成 Windows 服务，一键恢复"
                  : "选好程序，桌面与开始菜单快捷方式一键生成"
          }}
        </p>
      </div>
    </div>

    <div class="header-meta">
      <nav class="view-switch" aria-label="切换页面">
        <button
          class="view-tab"
          :class="{ active: view === 'processes' }"
          type="button"
          title="进程管理：批量结束 WebView2、node 等多实例进程"
          @click="emit('switch', 'processes')"
        >
          <svg viewBox="0 0 18 18" width="15" height="15" aria-hidden="true">
            <rect x="2" y="3" width="14" height="11" rx="2.4" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M2 6.4h14M4 4.6h.1M6 4.6h.1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            <path d="M5 9h5M5 11.2h7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" opacity="0.7" />
          </svg>
          进程管理
        </button>
        <button
          class="view-tab"
          :class="{ active: view === 'environment' }"
          type="button"
          title="环境变量管理：可视化编辑用户和系统环境变量"
          @click="emit('switch', 'environment')"
        >
          <svg viewBox="0 0 18 18" width="15" height="15" aria-hidden="true">
            <path d="M2.5 5h7M13.5 5h2M2.5 13h2M8.5 13h7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            <circle cx="11.5" cy="5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.5" />
            <circle cx="6.5" cy="13" r="1.8" fill="none" stroke="currentColor" stroke-width="1.5" />
          </svg>
          环境变量
        </button>
        <button
          class="view-tab"
          :class="{ active: view === 'services' }"
          type="button"
          title="服务管理：把任意 exe 注册为 Windows 服务，崩溃自动重启"
          @click="emit('switch', 'services')"
        >
          <svg viewBox="0 0 18 18" width="15" height="15" aria-hidden="true">
            <rect x="2.5" y="2.5" width="13" height="13" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" />
            <circle cx="6.5" cy="6.5" r="1.3" fill="currentColor" />
            <circle cx="11.5" cy="6.5" r="1.3" fill="currentColor" />
            <circle cx="6.5" cy="11.5" r="1.3" fill="currentColor" />
            <circle cx="11.5" cy="11.5" r="1.3" fill="currentColor" />
          </svg>
          服务管理
        </button>
        <button
          class="view-tab"
          :class="{ active: view === 'shortcuts' }"
          type="button"
          title="快捷方式：桌面与开始菜单快捷方式管理"
          @click="emit('switch', 'shortcuts')"
        >
          <svg viewBox="0 0 18 18" width="15" height="15" aria-hidden="true">
            <rect x="2.5" y="4" width="6.5" height="10" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M11.5 6.5h2.2a2 2 0 0 1 2 2v1a2 2 0 0 1-2 2h-2.2M7.5 9h3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          快捷方式
        </button>
      </nav>
      <span class="chip chip-scope">{{
        view === "environment" || view === "services" ? "Windows · 本机" : "Windows · 当前用户"
      }}</span>
      <span
        class="chip chip-accent"
        :class="{ 'chip-placeholder': view !== 'shortcuts' }"
        :aria-hidden="view !== 'shortcuts'"
      >
        已加入 {{ included }} / {{ total }}
      </span>
    </div>
  </header>
</template>

<style scoped>
.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 22px 24px 18px;
  flex: none;
}

.brand {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
}

.brand-mark {
  flex: none;
  width: 46px;
  height: 46px;
  border-radius: 14px;
  display: grid;
  place-items: center;
  background: #ffffff;
  border: 1px solid var(--line);
  box-shadow: 0 8px 18px -12px rgba(15, 108, 189, 0.6);
}

.brand-copy {
  min-width: 0;
}

.brand-copy h1 {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 21px;
  font-weight: 700;
  letter-spacing: -0.01em;
  color: var(--text);
}

.brand-en {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-3);
}

.brand-copy p {
  margin-top: 3px;
  font-size: 13px;
  color: var(--text-2);
}

.header-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: none;
}

.view-switch {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.66);
}

.view-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
  transition: border-color 0.16s ease, color 0.16s ease, background 0.16s ease, box-shadow 0.16s ease;
}

.view-tab:hover {
  color: var(--accent-strong);
  background: #edf5fd;
}

.view-tab.active {
  color: var(--accent-strong);
  border-color: #c9dcef;
  background: #fff;
  box-shadow: 0 1px 2px rgba(16, 24, 40, 0.07);
}

.view-tab svg {
  flex: none;
  color: var(--accent);
}

.view-tab.active svg {
  color: var(--accent-strong);
}

.chip {
  display: inline-flex;
  align-items: center;
  height: 28px;
  padding: 0 12px;
  border-radius: 999px;
  border: 1px solid var(--line);
  background: rgba(255, 255, 255, 0.7);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 500;
}

.chip-accent {
  border-color: #bcd8ef;
  background: var(--accent-soft);
  color: var(--accent-strong);
  font-weight: 600;
}

.chip-scope {
  min-width: 138px;
  justify-content: center;
}

.chip-placeholder {
  visibility: hidden;
}
</style>
