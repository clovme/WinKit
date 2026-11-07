import { invoke } from "@tauri-apps/api/core";
import type {
  ClearResult,
  EnvironmentProfile,
  EnvironmentProfileStore,
  EnvironmentProfileVariable,
  EnvironmentScope,
  EnvironmentSnapshot,
  EnvironmentVariable,
  KillProcessesResult,
  LibraryFilePayload,
  PickedProgram,
  ProcessSnapshot,
  ProgramIcon,
  ShortcutRequestItem,
  ShortcutResult,
  ShortcutState,
  TargetKind,
  UserScope,
} from "../types";

export const TARGET_LABELS: Record<TargetKind, string> = {
  desktop: "桌面",
  startmenu: "开始菜单",
};

export const SCOPE_LABELS: Record<UserScope, string> = {
  current: "当前用户",
  all: "所有用户",
};

/** 是否运行在 Tauri 桌面窗口里（浏览器预览时为 false） */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 调用 Rust 侧的系统文件选择窗口（IFileOpenDialog），支持一次多选 .exe。
 * 用户取消时返回空数组，不视为错误。
 */
export async function pickPrograms(): Promise<PickedProgram[]> {
  if (!isTauri()) {
    throw new Error("当前是浏览器预览，系统文件选择窗口只会在桌面应用中弹出");
  }
  return invoke<PickedProgram[]>("pick_programs");
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) {
    throw new Error("当前是浏览器预览，系统文件夹选择窗口只会在桌面应用中弹出");
  }
  return invoke<string | null>("pick_folder");
}

/**
 * 在桌面 / 开始菜单写入 .lnk。
 * Rust 侧对已存在的同名快捷方式返回 skipped（overwrite 为 true 时才覆盖）。
 */
export async function createShortcuts(
  items: ShortcutRequestItem[],
  overwrite: boolean
): Promise<ShortcutResult[]> {
  if (isTauri()) {
    return invoke<ShortcutResult[]>("create_shortcuts", { items, overwrite });
  }

  // 浏览器预览：模拟耗时与结果，方便先看交互，不写入任何文件
  await new Promise((resolve) => setTimeout(resolve, 420 + Math.random() * 260));
  return items.flatMap((item) =>
    item.targets.map((target) => ({
      program: item.name,
      target,
      status: "created" as const,
      message: null,
      linkPath: null,
    }))
  );
}

/**
 * 读取磁盘上真实的 .lnk 状态：exists / conflict / missing / error。
 * 前端不再用 JSON 里的 createdTargets 判断“已存在”。
 */
export async function inspectShortcuts(
  items: ShortcutRequestItem[]
): Promise<ShortcutState[]> {
  if (!isTauri() || !items.length) return [];
  return invoke<ShortcutState[]>("inspect_shortcuts", { items });
}

/**
 * 清除清单对应的真实快捷方式。Rust 侧只删除同名且 TargetPath 与程序一致的 .lnk，
 * 指向其他程序的文件不会删除。
 */
export async function clearShortcuts(
  items: ShortcutRequestItem[],
  targets: TargetKind[]
): Promise<ClearResult> {
  if (!isTauri()) {
    // 浏览器预览没有真实文件，只清空模拟标记
    return { deleted: 0, failed: 0, errors: [] };
  }
  return invoke<ClearResult>("clear_shortcuts", { items, targets });
}

/**
 * 批量读取程序图标。Rust 侧用 IShellItemImageFactory 取 64px 图标并转成 PNG data URL。
 * 浏览器预览没有本地程序，返回空列表。
 */
export async function loadProgramIcons(paths: string[]): Promise<ProgramIcon[]> {
  if (!isTauri() || !paths.length) return [];
  return invoke<ProgramIcon[]>("program_icons", { paths });
}

/**
 * 读取清单文件。Rust 侧优先读 exe 同目录的 WinKit.json，
 * 读不到时回落到 %APPDATA%\AppShortcut\WinKit.json。
 */
export async function loadLibrary(): Promise<LibraryFilePayload | null> {
  if (!isTauri()) return null;
  return invoke<LibraryFilePayload>("load_library");
}

/**
 * 保存清单文件，优先写 exe 同目录；程序目录不可写时 Rust 自动回落。
 * 返回实际写入路径与是否 portable。
 */
export async function saveLibrary(content: string): Promise<LibraryFilePayload> {
  if (!isTauri()) {
    throw new Error("浏览器预览没有本地清单文件");
  }
  return invoke<LibraryFilePayload>("save_library", { content });
}

/**
 * 读取当前用户能看到的所有进程。Rust 侧用 Toolhelp 快照拿 PID / 父 PID / 名称，
 * 再按进程句柄补路径、工作集内存和线程数。
 */
export async function listProcesses(): Promise<ProcessSnapshot> {
  if (!isTauri()) {
    return { processes: [], currentPid: 0 };
  }
  return invoke<ProcessSnapshot>("list_processes");
}

/**
 * 结束指定 PID。includeTree 为 true 时会先收集子进程，按子进程 → 父进程的顺序结束。
 * Rust 侧会跳过自身、PID 0/4 和 Windows 关键进程，并把失败原因带回前端。
 */
export async function killProcesses(
  pids: number[],
  includeTree: boolean
): Promise<KillProcessesResult> {
  if (!isTauri()) {
    throw new Error("浏览器预览无法结束真实进程");
  }
  return invoke<KillProcessesResult>("kill_processes", { pids, includeTree });
}

const MOCK_ENVIRONMENT: EnvironmentSnapshot = {
  user: {
    writable: true,
    error: null,
    variables: [
      { name: "GOPATH", value: "D:\\develop\\gopath", expand: false },
      { name: "JAVA_HOME", value: "D:\\application\\jdk-21", expand: false },
      { name: "Path", value: "%USERPROFILE%\\AppData\\Local\\Microsoft\\WindowsApps", expand: true },
      { name: "TEMP", value: "%USERPROFILE%\\AppData\\Local\\Temp", expand: true },
    ],
  },
  system: {
    writable: false,
    error: null,
    variables: [
      { name: "ComSpec", value: "%SystemRoot%\\system32\\cmd.exe", expand: true },
      { name: "DriverData", value: "C:\\Windows\\System32\\Drivers\\DriverData", expand: false },
      { name: "JAVA_HOME", value: "D:\\application\\jdk-21", expand: false },
      { name: "Path", value: "%SystemRoot%;%SystemRoot%\\System32", expand: true },
    ],
  },
};

/**
 * 读取用户 / 系统环境变量。浏览器预览返回示例数据，方便检查页面布局。
 */
export async function loadEnvironmentVariables(): Promise<EnvironmentSnapshot> {
  if (!isTauri()) {
    return JSON.parse(JSON.stringify(MOCK_ENVIRONMENT)) as EnvironmentSnapshot;
  }
  return invoke<EnvironmentSnapshot>("load_environment_variables");
}

/**
 * 新增或修改环境变量。originalName 有值时表示改名；expand 控制是否写 REG_EXPAND_SZ。
 * 系统变量需要管理员权限，失败时 Rust 会返回可读的错误原因。
 */
export async function saveEnvironmentVariable(
  scope: EnvironmentScope,
  originalName: string | null,
  name: string,
  value: string,
  expand: boolean
): Promise<EnvironmentVariable> {
  if (!isTauri()) {
    throw new Error("浏览器预览无法修改 Windows 环境变量");
  }
  return invoke<EnvironmentVariable>("save_environment_variable", {
    scope,
    originalName,
    name,
    value,
    expand,
  });
}

export async function deleteEnvironmentVariable(
  scope: EnvironmentScope,
  name: string
): Promise<void> {
  if (!isTauri()) {
    throw new Error("浏览器预览无法删除 Windows 环境变量");
  }
  return invoke<void>("delete_environment_variable", { scope, name });
}

const MOCK_ENVIRONMENT_PROFILES: EnvironmentProfile[] = [
  {
    id: "mock-java",
    name: "JAVA",
    note: "Java 开发环境",
    enabled: true,
    variables: [
      { id: "mock-java-home", name: "JAVA_HOME", value: "D:\\application\\jdk-21", scope: "system", expand: true, append: false },
      {
        id: "mock-java-classpath",
        name: "CLASSPATH",
        value: ".;%JAVA_HOME%\\lib\\dt.jar;%JAVA_HOME%\\lib\\tools.jar;",
        scope: "system",
        expand: true,
        append: false,
      },
      {
        id: "mock-java-path",
        name: "Path",
        value: "%JAVA_HOME%\\bin;%JAVA_HOME%\\jre\\bin",
        scope: "system",
        expand: true,
        append: true,
      },
    ],
  },
  {
    id: "mock-go",
    name: "Go",
    note: "Go 开发环境",
    enabled: true,
    variables: [
      { id: "mock-go-path-var", name: "GOPATH", value: "%USERPROFILE%\\go", scope: "user", expand: true, append: false },
      { id: "mock-go-path", name: "Path", value: "C:\\Program Files\\Go\\bin", scope: "user", expand: true, append: true },
      {
        id: "mock-go-zone",
        name: "ZONEINFO",
        value: "C:\\Program Files\\Go\\lib\\time\\zoneinfo.zip",
        scope: "system",
        expand: false,
        append: false,
      },
    ],
  },
];

const MOCK_PROFILE_STORE: EnvironmentProfileStore = {
  profiles: MOCK_ENVIRONMENT_PROFILES,
  path: "浏览器预览（不会写入文件）",
  portable: false,
};

function cloneProfiles(profiles: EnvironmentProfile[]): EnvironmentProfile[] {
  return JSON.parse(JSON.stringify(profiles)) as EnvironmentProfile[];
}

function parseProfileContent(content: string): EnvironmentProfile[] {
  if (!content.trim()) return [];
  const parsed = JSON.parse(content) as { profiles?: unknown };
  if (!parsed || !Array.isArray(parsed.profiles)) return [];
  return parsed.profiles
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === "object")
    .map((item) => {
      const variables = Array.isArray(item.variables) ? item.variables : [];
      return {
        id: String(item.id ?? ""),
        name: String(item.name ?? "未命名方案"),
        note: String(item.note ?? ""),
        enabled: item.enabled !== false,
        variables: variables
          .filter((variable): variable is Record<string, unknown> => Boolean(variable) && typeof variable === "object")
          .map((variable) => ({
            id: String(variable.id ?? ""),
            name: String(variable.name ?? ""),
            value: String(variable.value ?? ""),
            scope: (variable.scope === "user" ? "user" : "system") as EnvironmentScope,
            expand: variable.expand !== false,
            append: variable.append !== false,
          })),
      };
    })
    .filter((profile) => profile.id && profile.name);
}

/**
 * 读取应用里保存的环境变量方案（JAVA / Go ...）。
 * 方案本身只是 key:value 定义，是否已写入以注册表为准，不保存“已设置”状态。
 */
export async function loadEnvironmentProfiles(): Promise<EnvironmentProfileStore> {
  if (!isTauri()) {
    return {
      profiles: cloneProfiles(MOCK_PROFILE_STORE.profiles),
      path: MOCK_PROFILE_STORE.path,
      portable: false,
    };
  }
  const file = await invoke<LibraryFilePayload>("load_environment_profiles");
  return {
    profiles: parseProfileContent(file.content),
    path: file.path,
    portable: file.portable,
  };
}

/**
 * 保存环境变量方案。只写方案 JSON，不碰注册表。
 */
export async function saveEnvironmentProfiles(
  profiles: EnvironmentProfile[]
): Promise<EnvironmentProfileStore> {
  if (!isTauri()) {
    return {
      profiles: cloneProfiles(profiles),
      path: MOCK_PROFILE_STORE.path,
      portable: false,
    };
  }
  const content = JSON.stringify({ version: 1, profiles });
  const file = await invoke<LibraryFilePayload>("save_environment_profiles", { content });
  return {
    profiles: cloneProfiles(profiles),
    path: file.path,
    portable: file.portable,
  };
}

export type { EnvironmentProfileVariable };

// ---------- 服务管理 ----------

import type { ServiceConfigFile, ServiceDef, ServiceState } from "../types";

const MOCK_SERVICE_CONFIG: ServiceConfigFile = {
  version: 1,
  services: [
    {
      id: "mock-bililive",
      name: "WinKit-bililive",
      displayName: "B站直播录制",
      exePath: "D:\\application\\WTools\\Service\\bililive\\bililive-windows-amd64.exe",
      args: "--config=D:\\application\\WTools\\data\\bililive\\config.yml",
      workingDir: "D:\\application\\WTools\\Service\\bililive",
      autostart: true,
      restartOnCrash: true,
      restartDelaySecs: 5,
      logFile: "D:\\application\\WTools\\data\\bililive\\service.log",
    },
  ],
};

function cloneServices(services: ServiceDef[]): ServiceDef[] {
  return JSON.parse(JSON.stringify(services)) as ServiceDef[];
}

/** 读取服务配置文件（WinKit-services.json）。 */
export async function loadServiceConfigs(): Promise<ServiceConfigFile> {
  if (!isTauri()) {
    return { version: 1, services: cloneServices(MOCK_SERVICE_CONFIG.services) };
  }
  return invoke<ServiceConfigFile>("load_service_configs");
}

/** 保存服务配置文件（不注册到 SCM，只写 JSON）。 */
export async function saveServiceConfigs(services: ServiceDef[]): Promise<string> {
  if (!isTauri()) {
    return "浏览器预览（不会写入文件）";
  }
  return invoke<string>("save_service_configs", { config: { version: 1, services } });
}

/** 把某个服务注册 / 重建为 Windows 服务。 */
export async function registerService(def: ServiceDef): Promise<void> {
  if (!isTauri()) throw new Error("浏览器预览：注册服务只在桌面应用中生效");
  await invoke("register_service_command", { def });
}

/** 停止并从 SCM 删除服务（配置文件保留）。 */
export async function unregisterService(name: string): Promise<void> {
  if (!isTauri()) throw new Error("浏览器预览：卸载服务只在桌面应用中生效");
  await invoke("unregister_service_command", { name });
}

export async function startService(name: string): Promise<void> {
  if (!isTauri()) throw new Error("浏览器预览：启动服务只在桌面应用中生效");
  await invoke("start_service_command", { name });
}

export async function stopService(name: string): Promise<void> {
  if (!isTauri()) throw new Error("浏览器预览：停止服务只在桌面应用中生效");
  await invoke("stop_service_command", { name });
}

export async function restartService(name: string): Promise<void> {
  if (!isTauri()) throw new Error("浏览器预览：重启服务只在桌面应用中生效");
  await invoke("restart_service_command", { name });
}

/** 查询单个服务在 SCM 里的状态。 */
export async function queryServiceStatus(name: string): Promise<ServiceState> {
  if (!isTauri()) return "not_installed";
  return invoke<ServiceState>("query_service_status_command", { name });
}

/** 一键把配置文件里的所有服务注册并启动（重装后恢复用）。 */
export async function applyAllServices(): Promise<string[]> {
  if (!isTauri()) throw new Error("浏览器预览：一键恢复只在桌面应用中生效");
  return invoke<string[]>("apply_all_services");
}

/** 抓取当前进程的关键环境变量快照（USERPROFILE / PATH / APPDATA 等）。 */
export async function captureRuntimeEnv(): Promise<Record<string, string>> {
  if (!isTauri()) return {};
  return invoke<Record<string, string>>("capture_runtime_env");
}
