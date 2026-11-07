//! Windows 服务管理：把任意 exe + 参数包装成系统服务。
//!
//! 设计：
//! - WinKit.exe 自己既是 UI 也是服务宿主。带 `--service <名字>` 启动时进入
//!   宿主模式，不再启动 Tauri 界面，而是向 SCM 报到并拉起目标子进程。
//! - 配置持久化到 exe 同目录的 `WinKit-services.json`，重装系统后拷回这个文件，
//!   在 UI 里点「全部注册并启动」即可一键恢复。
//! - 不依赖 NSSM / WinSW；崩溃重启通过 SCM 自带的 `sc failure` 完成。

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

const SERVICE_CONFIG_FILE_NAME: &str = "WinKit-services.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDef {
    /// 唯一 id（本地 UI 用）
    pub id: String,
    /// Windows 服务名（SCM 用，不能含空格）
    pub name: String,
    /// 显示名
    pub display_name: String,
    /// 目标 exe 绝对路径
    pub exe_path: String,
    /// 原始命令行参数（原样拼接）
    #[serde(default)]
    pub args: String,
    /// 工作目录，留空则取 WinKit.exe 所在目录
    #[serde(default)]
    pub working_dir: String,
    /// 开机自启
    #[serde(default = "default_true")]
    pub autostart: bool,
    /// 崩溃后由 SCM 自动重启
    #[serde(default = "default_true")]
    pub restart_on_crash: bool,
    /// 崩溃重启间隔（秒）
    #[serde(default = "default_restart_delay")]
    pub restart_delay_secs: u32,
    /// 可选：把 stdout/stderr 追加写入这个日志文件
    #[serde(default)]
    pub log_file: String,
    /// 启动子进程时注入的环境变量（保存时快照当前用户环境，解决 LocalSystem 下 USERPROFILE/PATH 不对的问题）
    #[serde(default)]
    pub env_override: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}
fn default_restart_delay() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfigFile {
    pub version: u32,
    pub services: Vec<ServiceDef>,
}

impl Default for ServiceConfigFile {
    fn default() -> Self {
        Self { version: 1, services: Vec::new() }
    }
}

fn exe_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位程序目录：{e}"))?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| "无法定位程序目录".to_string())
}

fn config_path() -> Result<PathBuf, String> {
    Ok(exe_dir()?.join(SERVICE_CONFIG_FILE_NAME))
}

fn read_config_at(path: &Path) -> Result<ServiceConfigFile, String> {
    if !path.exists() {
        return Ok(ServiceConfigFile::default());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 {SERVICE_CONFIG_FILE_NAME} 失败：{e}"))?;
    if text.trim().is_empty() {
        return Ok(ServiceConfigFile::default());
    }
    let parsed: ServiceConfigFile = serde_json::from_str(&text)
        .map_err(|e| format!("{SERVICE_CONFIG_FILE_NAME} 不是合法 JSON：{e}"))?;
    Ok(parsed)
}

fn write_config_at(path: &Path, config: &ServiceConfigFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}：{e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(config)
        .map_err(|e| format!("序列化服务配置失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入临时配置失败：{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("保存服务配置失败：{e}"))?;
    Ok(())
}

// ---------- sc.exe 封装 ----------

fn run_sc(args: &[&str]) -> Result<String, String> {
    let output = Command::new("sc.exe")
        .args(args)
        .output()
        .map_err(|e| format!("调用 sc.exe 失败：{e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let combined = format!("{stdout}{stderr}").trim().to_string();
        return Err(if combined.is_empty() {
            format!("sc.exe 退出码 {:?}", output.status.code())
        } else {
            combined
        });
    }
    Ok(stdout)
}

/// 查询单个服务状态。
pub fn query_service_status(name: &str) -> Result<String, String> {
    let out = match run_sc(&["query", name]) {
        Ok(text) => text,
        Err(err) => {
            if err.contains("1060") || err.contains("does not exist") || err.contains("不存在") {
                return Ok("not_installed".to_string());
            }
            return Err(err);
        }
    };
    for line in out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("STATE") {
            let rest = rest.trim_start_matches(':').trim();
            let mut parts = rest.split_whitespace();
            let _num = parts.next();
            let word = parts.next().unwrap_or("").to_uppercase();
            return Ok(match word.as_str() {
                "RUNNING" => "running",
                "STOPPED" => "stopped",
                "START_PENDING" => "start_pending",
                "STOP_PENDING" => "stop_pending",
                "PAUSED" => "paused",
                _ => "unknown",
            }
            .to_string());
        }
    }
    Ok("unknown".to_string())
}

fn kill_child_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// 注册（或重建）服务。已存在时先停止并删除，再重新创建，保证配置最新。
pub fn register_service(def: &ServiceDef) -> Result<(), String> {
    if def.name.trim().is_empty() {
        return Err("服务名不能为空".to_string());
    }
    if def.exe_path.trim().is_empty() {
        return Err("目标 exe 路径不能为空".to_string());
    }
    if !Path::new(&def.exe_path).is_file() {
        return Err(format!("目标 exe 不存在：{}", def.exe_path));
    }

    if query_service_status(&def.name)? != "not_installed" {
        let _ = stop_service(&def.name);
        std::thread::sleep(Duration::from_millis(600));
        let _ = run_sc(&["delete", &def.name]);
        std::thread::sleep(Duration::from_millis(300));
    }

    let exe = std::env::current_exe().map_err(|e| format!("定位 WinKit.exe 失败：{e}"))?;
    let bin_path = format!("\"{}\" --service {}", exe.display(), def.name);
    let start = if def.autostart { "auto" } else { "demand" };

    run_sc(&[
        "create",
        &def.name,
        "binPath=",
        &bin_path,
        "start=",
        start,
        "displayname=",
        &def.display_name,
    ])?;

    let _ = run_sc(&["description", &def.name, &def.display_name]);

    if def.restart_on_crash {
        let delay_ms = (def.restart_delay_secs.max(1) * 1000).to_string();
        let actions = format!("restart/{delay_ms}/restart/{delay_ms}/restart/{delay_ms}");
        let _ = run_sc(&["failure", &def.name, "reset=", "86400", "actions=", &actions]);
    } else {
        let _ = run_sc(&["failure", &def.name, "reset=", "0", "actions=", ""]);
    }

    Ok(())
}

pub fn start_service(name: &str) -> Result<(), String> {
    run_sc(&["start", name]).map(|_| ())
}

pub fn stop_service(name: &str) -> Result<(), String> {
    run_sc(&["stop", name]).map(|_| ())
}

pub fn restart_service(name: &str) -> Result<(), String> {
    let _ = stop_service(name);
    for _ in 0..10 {
        if let Ok(state) = query_service_status(name) {
            if state == "stopped" || state == "not_installed" {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    start_service(name)
}

pub fn unregister_service(name: &str) -> Result<(), String> {
    let _ = stop_service(name);
    std::thread::sleep(Duration::from_millis(500));
    run_sc(&["delete", name]).map(|_| ())
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub fn load_service_configs() -> Result<ServiceConfigFile, String> {
    read_config_at(&config_path()?)
}

#[tauri::command]
pub fn save_service_configs(config: ServiceConfigFile) -> Result<String, String> {
    let path = config_path()?;
    write_config_at(&path, &config)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn register_service_command(def: ServiceDef) -> Result<(), String> {
    register_service(&def)
}

#[tauri::command]
pub fn unregister_service_command(name: String) -> Result<(), String> {
    unregister_service(&name)
}

#[tauri::command]
pub fn start_service_command(name: String) -> Result<(), String> {
    start_service(&name)
}

#[tauri::command]
pub fn stop_service_command(name: String) -> Result<(), String> {
    stop_service(&name)
}

#[tauri::command]
pub fn restart_service_command(name: String) -> Result<(), String> {
    restart_service(&name)
}

#[tauri::command]
pub fn query_service_status_command(name: String) -> Result<String, String> {
    query_service_status(&name)
}

#[tauri::command]
pub fn apply_all_services() -> Result<Vec<String>, String> {
    let config = read_config_at(&config_path()?)?;
    let mut results = Vec::new();
    for def in &config.services {
        match register_service(def).and_then(|_| start_service(&def.name)) {
            Ok(()) => results.push(format!("{}：已注册并启动", def.display_name)),
            Err(e) => results.push(format!("{}：{}", def.display_name, e)),
        }
    }
    Ok(results)
}

/// 抓取当前进程的关键环境变量（用户身份、profile、PATH、临时目录等），
/// 让 UI 在保存服务配置时把这份快照写进 JSON；宿主以 LocalSystem 拉起子进程时再注入回去，
/// 解决服务模式下 USERPROFILE 指向 systemprofile、找不到 ffmpeg / 用户 cookie 等问题。
#[tauri::command]
pub fn capture_runtime_env() -> HashMap<String, String> {
    const KEEP: &[&str] = &[
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "USERNAME",
        "USERDOMAIN",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
        "PATH",
        "PATHEXT",
        "COMSPEC",
        "SystemRoot",
        "SystemDrive",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "NUMBER_OF_PROCESSORS",
        "OS",
        "PROCESSOR_ARCHITECTURE",
    ];
    std::env::vars()
        .filter(|(k, _)| KEEP.contains(&k.as_str()))
        .collect()
}

// ---------- 服务宿主（WinKit.exe --service <name>） ----------

/// 如果命令行带 `--service <名字>`，则进入服务宿主模式并返回 true；否则返回 false。
pub fn maybe_run_service_host() -> bool {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let mut expecting_name = false;
    let mut service_name: Option<String> = None;
    for arg in args {
        let lossy = arg.to_string_lossy().into_owned();
        if expecting_name {
            service_name = Some(lossy);
            break;
        }
        if lossy == "--service" {
            expecting_name = true;
        } else {
            return false;
        }
    }
    let Some(name) = service_name else {
        return false;
    };
    let _ = run_service_host(&name);
    true
}

fn load_def_for_host(service_name: &str) -> Result<ServiceDef, String> {
    let path = config_path()?;
    let config = read_config_at(&path)?;
    config
        .services
        .into_iter()
        .find(|def| def.name == service_name)
        .ok_or_else(|| format!("在 {} 里找不到服务 {service_name}", path.display()))
}

/// 服务宿主模式下当前要运行的服务名，由 main 在派发前写入。
static HOST_SERVICE_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn run_service_host(service_name: &str) -> Result<(), String> {
    let _ = HOST_SERVICE_NAME.set(service_name.to_string());
    service_dispatcher::start(service_name, ffi_service_main)
        .map_err(|e| format!("SCM 分发失败：{e}"))
}

// 生成 Windows 服务入口的样板函数（解析 argc/argv 为 Vec<OsString>）。
define_windows_service!(ffi_service_main, my_service_main);

fn my_service_main(_arguments: Vec<OsString>) {
    let name = HOST_SERVICE_NAME
        .get()
        .cloned()
        .unwrap_or_else(|| "WinKit-host".to_string());
    let _ = host_main(&name);
}

fn host_main(service_name: &str) -> Result<(), windows_service::Error> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let status_handle = service_control_handler::register(service_name, move |control| {
        match control {
            ServiceControl::Stop => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;

    let mut status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(3),
        process_id: None,
    };
    status_handle.set_service_status(status.clone())?;

    let def = match load_def_for_host(service_name) {
        Ok(d) => d,
        Err(e) => {
            log_host_error(service_name, &e);
            status.current_state = ServiceState::Stopped;
            status.exit_code = ServiceExitCode::Win32(1);
            let _ = status_handle.set_service_status(status);
            return Ok(());
        }
    };

    let child = spawn_child(&def);
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            log_host_error(service_name, &format!("启动子进程失败：{e}"));
            status.current_state = ServiceState::Stopped;
            status.exit_code = ServiceExitCode::Win32(1);
            let _ = status_handle.set_service_status(status);
            return Ok(());
        }
    };

    status.current_state = ServiceState::Running;
    status_handle.set_service_status(status.clone())?;

    loop {
        match shutdown_rx.recv_timeout(Duration::from_millis(400)) {
            Ok(()) => {
                kill_child_tree(child.id());
                let _ = child.wait();
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        status.current_state = ServiceState::Stopped;
                        status.exit_code = match exit_status.code() {
                            Some(code) => ServiceExitCode::ServiceSpecific(code as u32),
                            None => ServiceExitCode::Win32(1),
                        };
                        let _ = status_handle.set_service_status(status);
                        return Ok(());
                    }
                    Ok(None) => continue,
                    Err(_) => break,
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    status.current_state = ServiceState::Stopped;
    status.exit_code = ServiceExitCode::Win32(0);
    let _ = status_handle.set_service_status(status);
    Ok(())
}

#[cfg(windows)]
fn spawn_child(def: &ServiceDef) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(&def.exe_path);
    if !def.args.trim().is_empty() {
        // Windows 下直接把原始命令行接到 exe 后面，保留用户写的引号与 = 号
        cmd.raw_arg(&format!(
            " {}",
            def.args.trim_start()
        ));
    }
    if !def.working_dir.trim().is_empty() {
        cmd.current_dir(&def.working_dir);
    } else if let Some(dir) = Path::new(&def.exe_path).parent() {
        // 留空：默认用目标 exe 所在目录，而不是 WinKit.exe 所在目录
        if !dir.as_os_str().is_empty() {
            cmd.current_dir(dir);
        }
    }
    if !def.log_file.trim().is_empty() {
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&def.log_file)
        {
            cmd.stdout(Stdio::from(file.try_clone()?));
            cmd.stderr(Stdio::from(file));
        }
    }
    if !def.env_override.is_empty() {
        cmd.envs(def.env_override.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    cmd.spawn()
}

#[cfg(not(windows))]
fn spawn_child(_def: &ServiceDef) -> std::io::Result<std::process::Child> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "仅 Windows 支持服务宿主"))
}

fn log_host_error(service_name: &str, message: &str) {
    let Ok(path) = exe_dir() else { return };
    let log = path.join("WinKit-service-host.log");
    let existing = std::fs::read_to_string(&log).unwrap_or_default();
    let line = format!("{existing}{message}\t[{service_name}]\n");
    let _ = std::fs::write(&log, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_config_round_trip() {
        let cfg = ServiceConfigFile {
            version: 1,
            services: vec![ServiceDef {
                id: "s1".into(),
                name: "WinKit-bililive".into(),
                display_name: "bili live".into(),
                exe_path: r"D:\app\svc.exe".into(),
                args: "--config=D:\\app\\config.yml".into(),
                working_dir: "".into(),
                autostart: true,
                restart_on_crash: true,
                restart_delay_secs: 5,
                log_file: "".into(),
            }],
        };
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        let back: ServiceConfigFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.services[0].name, "WinKit-bililive");
        assert_eq!(back.services[0].args, "--config=D:\\app\\config.yml");
    }

    #[test]
    fn parses_state_running() {
        let out = "STATE              : 4  RUNNING";
        let mut parts = out.trim().strip_prefix("STATE").unwrap().trim_start_matches(':').trim().split_whitespace();
        let _num = parts.next();
        assert_eq!(parts.next().unwrap().to_uppercase(), "RUNNING");
    }
}
