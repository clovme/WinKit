export type TargetKind = "desktop" | "startmenu";

export type UserScope = "current" | "all";

export type ShortcutStatus = "created" | "skipped" | "failed";

export type ShortcutStateKind = "exists" | "conflict" | "missing" | "error";

/** 默认不分组：开始菜单快捷方式直接放在 Programs 根目录；需要时按行单独设置 */
export const DEFAULT_START_MENU_GROUP = "";

/** 旧版本曾把“默认分组”写成真实文件夹，迁移时按不分组处理 */
export const LEGACY_START_MENU_GROUP = "默认分组";

export interface CreateTargets {
  desktop: boolean;
  startmenu: boolean;
}

/** 新加入程序的默认创建位置：桌面 + 开始菜单 */
export const DEFAULT_CREATE_TARGETS: CreateTargets = { desktop: true, startmenu: true };

export interface LibraryOptions {
  overwrite: boolean;
}

export interface ProgramItem {
  id: string;
  name: string;
  path: string;
  color: string;
  included: boolean;
  scope: UserScope;
  group: string;
  targets: CreateTargets;
  createdTargets: TargetKind[];
  skippedTargets: TargetKind[];
  creating: boolean;
  failed: boolean;
  errorMessage: string | null;
}

export interface PickedProgram {
  name: string;
  path: string;
}

export interface ProgramIcon {
  path: string;
  icon: string | null;
}

export interface LibraryFilePayload {
  path: string;
  content: string;
  portable: boolean;
}

export interface ShortcutRequestItem {
  name: string;
  path: string;
  scope: UserScope;
  group: string;
  targets: TargetKind[];
}

export interface ShortcutState {
  program: string;
  programPath: string;
  target: TargetKind;
  status: ShortcutStateKind;
  message?: string | null;
  linkPath?: string | null;
}

export interface ClearResult {
  deleted: number;
  failed: number;
  errors: string[];
}

export interface ProcessInfo {
  pid: number;
  parentPid: number;
  name: string;
  path: string | null;
  ports: number[];
  memoryKb: number;
  threadCount: number;
  protected: boolean;
  protectedReason: string | null;
}

export interface ProcessSnapshot {
  processes: ProcessInfo[];
  currentPid: number;
}

export interface KillProcessesResult {
  killed: number;
  failed: number;
  errors: string[];
}

export type EnvironmentScope = "user" | "system";

export interface EnvironmentVariable {
  name: string;
  value: string;
  expand: boolean;
}

export interface EnvironmentScopeSnapshot {
  variables: EnvironmentVariable[];
  writable: boolean;
  error: string | null;
}

export interface EnvironmentSnapshot {
  user: EnvironmentScopeSnapshot;
  system: EnvironmentScopeSnapshot;
}

export interface EnvironmentProfileVariable {
  id: string;
  name: string;
  value: string;
  scope: EnvironmentScope;
  expand: boolean;
  append: boolean;
}

export interface EnvironmentProfile {
  id: string;
  name: string;
  note: string;
  enabled: boolean;
  variables: EnvironmentProfileVariable[];
}

export interface EnvironmentProfileStore {
  profiles: EnvironmentProfile[];
  path: string;
  portable: boolean;
}

export interface ShortcutResult {
  program: string;
  target: TargetKind;
  status: ShortcutStatus;
  message?: string | null;
  linkPath?: string | null;
}

export interface ProgramPreset {
  name: string;
  path: string;
  color: string;
}

export type ToastTone = "success" | "info" | "danger";

export interface ToastMessage {
  id: number;
  text: string;
  tone: ToastTone;
}

export type ServiceState =
  | "running"
  | "stopped"
  | "start_pending"
  | "stop_pending"
  | "paused"
  | "not_installed"
  | "unknown";

export interface ServiceDef {
  id: string;
  /** Windows 服务名（SCM 用，不含空格） */
  name: string;
  displayName: string;
  exePath: string;
  /** 原始命令行参数，原样拼接 */
  args: string;
  workingDir: string;
  autostart: boolean;
  restartOnCrash: boolean;
  restartDelaySecs: number;
  logFile: string;
  /** 启动子进程时注入的环境变量快照（解决 LocalSystem 下 profile/PATH 不对的问题） */
  envOverride?: Record<string, string>;
}

export interface ServiceConfigFile {
  version: number;
  services: ServiceDef[];
}
