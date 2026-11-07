// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 带 `--service <名字>` 启动时，WinKit.exe 作为系统服务宿主运行，不再启动 UI。
    if shortcut_lib::service_manager::maybe_run_service_host() {
        return;
    }
    shortcut_lib::run()
}
