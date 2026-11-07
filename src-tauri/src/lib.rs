use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND,
    ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
    GetLastError, HWND, LPARAM, NO_ERROR, SIZE, SetLastError, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_CLASS, TCP_TABLE_OWNER_PID_LISTENER,
    UDP_TABLE_CLASS, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IBindCtx, IPersistFile, STGM_READ,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegDeleteValueW, RegEnumValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
    QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs,
    FOLDERID_PublicDesktop, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, IShellItemImageFactory,
    IShellLinkW, KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderPath,
    SIGDN_FILESYSPATH, SIIGBF, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_RESTORE, SendMessageTimeoutW,
    SetForegroundWindow, ShowWindow, WM_SETTINGCHANGE,
};
use windows::core::{GUID, Interface, PCWSTR, PWSTR, w};

pub mod service_manager;

const ERROR_CANCELLED_CODE: u32 = 0x8007_04C7;
const LIBRARY_FILE_NAME: &str = "WinKit.json";
const LEGACY_LIBRARY_FILE_NAME: &str = "shortcuts.json";
const ENVIRONMENT_PROFILE_FILE_NAME: &str = "environment-profiles.json";
const SINGLE_INSTANCE_MUTEX: &str = "Local\\AppShortcut_SingleInstance";
const WINDOW_TITLE: &str = "WinKit 工具箱";
const USER_ENV_KEY: &str = "Environment";
const SYSTEM_ENV_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutItem {
    name: String,
    path: String,
    targets: Vec<String>,
    scope: String,
    #[serde(default)]
    group: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResult {
    program: String,
    target: String,
    status: &'static str,
    message: Option<String>,
    link_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutState {
    program: String,
    program_path: String,
    target: String,
    status: &'static str,
    message: Option<String>,
    link_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearResult {
    deleted: usize,
    failed: usize,
    errors: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedProgram {
    name: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramIcon {
    path: String,
    icon: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFile {
    path: String,
    content: String,
    portable: bool,
}

fn exe_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| format!("无法定位程序目录：{error}"))?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法定位程序目录".to_string())
}

fn portable_library_path() -> Result<PathBuf, String> {
    Ok(exe_dir()?.join(LIBRARY_FILE_NAME))
}

fn legacy_portable_library_path() -> Result<PathBuf, String> {
    Ok(exe_dir()?.join(LEGACY_LIBRARY_FILE_NAME))
}

fn fallback_library_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("AppShortcut").join(LIBRARY_FILE_NAME)
}

fn legacy_fallback_library_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("AppShortcut").join(LEGACY_LIBRARY_FILE_NAME)
}

fn read_library_at(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取清单 {} 失败：{error}", path.display()))?;
    if serde_json::from_str::<serde_json::Value>(&text).is_err() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let backup = path.with_extension(format!("invalid-{stamp}.json"));
        let _ = std::fs::rename(path, &backup);
        return Ok(None);
    }
    Ok(Some(text))
}

fn read_and_migrate_legacy_library(legacy: &Path, target: &Path) -> Result<Option<String>, String> {
    let Some(content) = read_library_at(legacy)? else {
        return Ok(None);
    };
    let _ = std::fs::rename(legacy, target);
    Ok(Some(content))
}

fn write_library_at(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建目录 {}：{error}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, content).map_err(|error| format!("写入清单临时文件失败：{error}"))?;
    std::fs::rename(&temp, path).map_err(|error| format!("保存清单失败：{error}"))?;
    Ok(())
}

fn load_library_blocking() -> Result<LibraryFile, String> {
    let portable_path = portable_library_path()?;
    if let Some(content) = read_library_at(&portable_path)? {
        return Ok(LibraryFile {
            path: portable_path.display().to_string(),
            content,
            portable: true,
        });
    }

    let legacy_portable = legacy_portable_library_path()?;
    if let Some(content) = read_and_migrate_legacy_library(&legacy_portable, &portable_path)? {
        return Ok(LibraryFile {
            path: portable_path.display().to_string(),
            content,
            portable: true,
        });
    }

    let fallback = fallback_library_path();
    if let Some(content) = read_library_at(&fallback)? {
        return Ok(LibraryFile {
            path: fallback.display().to_string(),
            content,
            portable: false,
        });
    }

    let legacy_fallback = legacy_fallback_library_path();
    if let Some(content) = read_and_migrate_legacy_library(&legacy_fallback, &fallback)? {
        return Ok(LibraryFile {
            path: fallback.display().to_string(),
            content,
            portable: false,
        });
    }

    Ok(LibraryFile {
        path: portable_path.display().to_string(),
        content: String::new(),
        portable: true,
    })
}

fn save_library_blocking(content: &str) -> Result<LibraryFile, String> {
    serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| format!("清单内容不是合法 JSON：{error}"))?;

    let portable_path = portable_library_path()?;
    let portable_error = match write_library_at(&portable_path, content) {
        Ok(()) => {
            return Ok(LibraryFile {
                path: portable_path.display().to_string(),
                content: content.to_string(),
                portable: true,
            });
        }
        Err(error) => error,
    };

    let fallback = fallback_library_path();
    write_library_at(&fallback, content).map_err(|error| {
        format!(
            "程序目录写入失败（{portable_error}），备用目录 {} 也写入失败（{error}）",
            fallback.display()
        )
    })?;
    Ok(LibraryFile {
        path: fallback.display().to_string(),
        content: content.to_string(),
        portable: false,
    })
}

#[tauri::command]
fn load_library() -> Result<LibraryFile, String> {
    load_library_blocking()
}

#[tauri::command]
fn save_library(content: String) -> Result<LibraryFile, String> {
    save_library_blocking(&content)
}

fn portable_environment_profile_path() -> Result<PathBuf, String> {
    Ok(exe_dir()?.join(ENVIRONMENT_PROFILE_FILE_NAME))
}

fn fallback_environment_profile_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("AppShortcut").join(ENVIRONMENT_PROFILE_FILE_NAME)
}

fn read_environment_profile_at(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取环境变量方案 {} 失败：{error}", path.display()))?;
    if serde_json::from_str::<serde_json::Value>(&text).is_err() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let backup = path.with_extension(format!("invalid-{stamp}.json"));
        let _ = std::fs::rename(path, &backup);
        return Ok(None);
    }
    Ok(Some(text))
}

fn write_environment_profile_at(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建目录 {}：{error}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, content)
        .map_err(|error| format!("写入环境变量方案临时文件失败：{error}"))?;
    std::fs::rename(&temp, path).map_err(|error| format!("保存环境变量方案失败：{error}"))?;
    Ok(())
}

fn load_environment_profiles_blocking() -> Result<LibraryFile, String> {
    let portable_path = portable_environment_profile_path()?;
    if let Some(content) = read_environment_profile_at(&portable_path)? {
        return Ok(LibraryFile {
            path: portable_path.display().to_string(),
            content,
            portable: true,
        });
    }

    let fallback = fallback_environment_profile_path();
    if let Some(content) = read_environment_profile_at(&fallback)? {
        return Ok(LibraryFile {
            path: fallback.display().to_string(),
            content,
            portable: false,
        });
    }

    Ok(LibraryFile {
        path: portable_path.display().to_string(),
        content: String::new(),
        portable: true,
    })
}

fn save_environment_profiles_blocking(content: &str) -> Result<LibraryFile, String> {
    serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| format!("环境变量方案内容不是合法 JSON：{error}"))?;

    let portable_path = portable_environment_profile_path()?;
    let portable_error = match write_environment_profile_at(&portable_path, content) {
        Ok(()) => {
            return Ok(LibraryFile {
                path: portable_path.display().to_string(),
                content: content.to_string(),
                portable: true,
            });
        }
        Err(error) => error,
    };

    let fallback = fallback_environment_profile_path();
    write_environment_profile_at(&fallback, content).map_err(|error| {
        format!(
            "程序目录写入失败（{portable_error}），备用目录 {} 也写入失败（{error}）",
            fallback.display()
        )
    })?;
    Ok(LibraryFile {
        path: fallback.display().to_string(),
        content: content.to_string(),
        portable: false,
    })
}

#[tauri::command]
fn load_environment_profiles() -> Result<LibraryFile, String> {
    load_environment_profiles_blocking()
}

#[tauri::command]
fn save_environment_profiles(content: String) -> Result<LibraryFile, String> {
    save_environment_profiles_blocking(&content)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if matches!(
                character,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "快捷方式".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)
            .map_err(|error| format!("无法获取系统目录：{error}"))?;
        let text = raw
            .to_string()
            .map_err(|error| format!("目录路径解析失败：{error}"))?;
        CoTaskMemFree(Some(raw.0 as *const c_void));
        Ok(PathBuf::from(text))
    }
}

fn sanitize_group(group: &str) -> String {
    if group.trim().is_empty() {
        return String::new();
    }
    let cleaned = sanitize_name(group);
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.chars().take(60).collect()
    }
}

fn folder_for(target: &str, scope: &str, group: &str) -> Result<PathBuf, String> {
    let id: &GUID = match (target, scope) {
        ("desktop", "all") => &FOLDERID_PublicDesktop,
        ("desktop", _) => &FOLDERID_Desktop,
        ("startmenu", "all") => &FOLDERID_CommonPrograms,
        ("startmenu", _) => &FOLDERID_Programs,
        _ => return Err(format!("未知的创建位置：{target}")),
    };
    let base = known_folder(id)?;
    if target == "startmenu" {
        let folder = sanitize_group(group);
        if folder.is_empty() {
            Ok(base)
        } else {
            Ok(base.join(folder))
        }
    } else {
        Ok(base)
    }
}

fn pick_programs_blocking(owner: Option<HWND>) -> Result<Vec<PickedProgram>, String> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = pick_programs_inner(owner);
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

unsafe fn pick_programs_inner(owner: Option<HWND>) -> Result<Vec<PickedProgram>, String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("无法打开文件选择窗口：{error}"))?;

        let options = FILEOPENDIALOGOPTIONS(
            FOS_ALLOWMULTISELECT.0 | FOS_FILEMUSTEXIST.0 | FOS_PATHMUSTEXIST.0,
        );
        dialog
            .SetOptions(options)
            .map_err(|error| format!("文件选择窗口初始化失败：{error}"))?;
        let _ = dialog.SetTitle(w!("选择要创建快捷方式的程序"));
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("程序 (*.exe)"),
            pszSpec: w!("*.exe"),
        }];
        let _ = dialog.SetFileTypes(&filters);
        let _ = dialog.SetFileTypeIndex(1);

        if let Err(error) = dialog.Show(owner) {
            if error.code().0 as u32 == ERROR_CANCELLED_CODE {
                return Ok(Vec::new());
            }
            return Err(format!("文件选择窗口打开失败：{error}"));
        }

        let results = dialog
            .GetResults()
            .map_err(|error| format!("读取选择结果失败：{error}"))?;
        let count = results
            .GetCount()
            .map_err(|error| format!("读取选择结果失败：{error}"))?;

        let mut picked = Vec::new();
        for index in 0..count {
            let item = results
                .GetItemAt(index)
                .map_err(|error| format!("读取选择结果失败：{error}"))?;
            let raw = item
                .GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|error| format!("读取文件路径失败：{error}"))?;
            let path = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as *const c_void));

            if let Some(path) = path {
                if path.is_empty() {
                    continue;
                }
                let name = Path::new(&path)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("程序")
                    .to_string();
                picked.push(PickedProgram { name, path });
            }
        }
        Ok(picked)
    }
}

fn pick_folder_blocking(owner: Option<HWND>) -> Result<Option<String>, String> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = pick_folder_inner(owner);
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

unsafe fn pick_folder_inner(owner: Option<HWND>) -> Result<Option<String>, String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("无法打开文件夹选择窗口：{error}"))?;
        let options = FILEOPENDIALOGOPTIONS(FOS_PICKFOLDERS.0 | FOS_PATHMUSTEXIST.0);
        dialog
            .SetOptions(options)
            .map_err(|error| format!("文件夹选择窗口初始化失败：{error}"))?;
        let _ = dialog.SetTitle(w!("选择文件夹"));

        if let Err(error) = dialog.Show(owner) {
            if error.code().0 as u32 == ERROR_CANCELLED_CODE {
                return Ok(None);
            }
            return Err(format!("文件夹选择窗口打开失败：{error}"));
        }

        let item = dialog
            .GetResult()
            .map_err(|error| format!("读取选择结果失败：{error}"))?;
        let raw = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|error| format!("读取文件夹路径失败：{error}"))?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const c_void));
        Ok(path.filter(|value| !value.is_empty()))
    }
}

unsafe fn write_link(exe: &Path, link_path: &Path, description: &str) -> Result<(), String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("创建快捷方式对象失败：{error}"))?;

        let exe_wide = wide(&exe.to_string_lossy());
        link.SetPath(PCWSTR::from_raw(exe_wide.as_ptr()))
            .map_err(|error| format!("设置程序路径失败：{error}"))?;

        if let Some(parent) = exe.parent() {
            let working_dir = wide(&parent.to_string_lossy());
            let _ = link.SetWorkingDirectory(PCWSTR::from_raw(working_dir.as_ptr()));
        }

        let description_wide = wide(description);
        let _ = link.SetDescription(PCWSTR::from_raw(description_wide.as_ptr()));
        let _ = link.SetIconLocation(PCWSTR::from_raw(exe_wide.as_ptr()), 0);

        let persist: IPersistFile = link
            .cast()
            .map_err(|error| format!("获取快捷方式保存接口失败：{error}"))?;
        let link_wide = wide(&link_path.to_string_lossy());
        persist
            .Save(PCWSTR::from_raw(link_wide.as_ptr()), true)
            .map_err(|error| format!("写入快捷方式失败：{error}"))
    }
}

fn normalize_path(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolved.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn same_target(shortcut_target: &Path, exe: &Path) -> bool {
    normalize_path(shortcut_target) == normalize_path(exe)
}

unsafe fn read_link_target(link_path: &Path) -> Option<PathBuf> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let target = read_link_target_inner(link_path);
        if init.is_ok() {
            CoUninitialize();
        }
        target
    }
}

unsafe fn read_link_target_inner(link_path: &Path) -> Option<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let path_wide = wide(&link_path.to_string_lossy());
        persist
            .Load(PCWSTR::from_raw(path_wide.as_ptr()), STGM_READ)
            .ok()?;

        let mut buffer = [0u16; 1024];
        link.GetPath(&mut buffer, std::ptr::null_mut(), 0).ok()?;
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let target = String::from_utf16_lossy(&buffer[..end]);
        if target.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(target))
        }
    }
}

unsafe fn hbitmap_to_data_url(bitmap: HBITMAP) -> Option<String> {
    unsafe {
        let mut info: BITMAP = std::mem::zeroed();
        let got_object = GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some((&mut info as *mut BITMAP).cast()),
        );
        if got_object == 0 {
            return None;
        }
        let width = info.bmWidth;
        let height = info.bmHeight.abs();
        if width <= 0 || height <= 0 || width > 512 || height > 512 {
            return None;
        }

        let mut header: BITMAPINFO = std::mem::zeroed();
        header.bmiHeader.biSize =
            size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
        header.bmiHeader.biWidth = width;
        header.bmiHeader.biHeight = -height;
        header.bmiHeader.biPlanes = 1;
        header.bmiHeader.biBitCount = 32;
        header.bmiHeader.biCompression = BI_RGB.0;

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let memory_dc = CreateCompatibleDC(None);
        let dib_result = GetDIBits(
            memory_dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(memory_dc);
        if dib_result == 0 {
            return None;
        }

        let all_transparent = pixels.chunks_exact(4).all(|pixel| pixel[3] == 0);
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            if all_transparent {
                pixel[3] = 255;
            }
        }

        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().ok()?;
            writer.write_image_data(&pixels).ok()?;
            writer.finish().ok()?;
        }
        Some(format!("data:image/png;base64,{}", base64_encode(&encoded)))
    }
}

unsafe fn icon_for_path(path: &str) -> Option<String> {
    unsafe {
        let wide_path = wide(path);
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR::from_raw(wide_path.as_ptr()), None::<&IBindCtx>)
                .ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let bitmap = factory
            .GetImage(
                SIZE { cx: 64, cy: 64 },
                SIIGBF(SIIGBF_ICONONLY.0 | SIIGBF_BIGGERSIZEOK.0),
            )
            .ok()?;
        let icon = hbitmap_to_data_url(bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        icon
    }
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(third & 0b11_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

#[tauri::command]
async fn program_icons(paths: Vec<String>) -> Vec<ProgramIcon> {
    tauri::async_runtime::spawn_blocking(move || unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut icons = Vec::with_capacity(paths.len());
        for path in paths {
            let icon = icon_for_path(&path);
            icons.push(ProgramIcon { path, icon });
        }
        if init.is_ok() {
            CoUninitialize();
        }
        icons
    })
    .await
    .unwrap_or_default()
}

fn create_one(item: &ShortcutItem, target: &str, overwrite: bool) -> CreateResult {
    let folder = match folder_for(target, &item.scope, &item.group) {
        Ok(folder) => folder,
        Err(error) => {
            return CreateResult {
                program: item.name.clone(),
                target: target.to_string(),
                status: "failed",
                message: Some(error),
                link_path: None,
            };
        }
    };
    create_one_in_folder(item, target, &folder, overwrite)
}

fn create_one_in_folder(
    item: &ShortcutItem,
    target: &str,
    folder: &Path,
    overwrite: bool,
) -> CreateResult {
    let result =
        |status: &'static str, message: Option<String>, link: Option<PathBuf>| CreateResult {
            program: item.name.clone(),
            target: target.to_string(),
            status,
            message,
            link_path: link.map(|path| path.to_string_lossy().into_owned()),
        };

    if let Err(error) = std::fs::create_dir_all(folder) {
        return result(
            "failed",
            Some(format!("无法创建目标目录 {}：{error}", folder.display())),
            None,
        );
    }

    let exe = Path::new(&item.path);
    if !exe.is_file() {
        return result(
            "failed",
            Some(format!("程序文件不存在：{}", item.path)),
            None,
        );
    }

    let name = sanitize_name(&item.name);
    let link_path = shortcut_path(folder, &item.name);

    if link_path.exists() {
        let existing = unsafe { read_link_target(&link_path) };
        let matches_target = existing
            .as_deref()
            .map(|existing_target| same_target(existing_target, exe))
            .unwrap_or(false);

        if matches_target {
            if !overwrite {
                return result(
                    "skipped",
                    Some("同名快捷方式已存在且指向同一程序，已跳过".to_string()),
                    Some(link_path),
                );
            }
        } else if !overwrite {
            let detail = existing
                .map(|existing_target| format!("，现有快捷方式指向 {}", existing_target.display()))
                .unwrap_or_else(|| "，无法读取现有快捷方式的目标".to_string());
            return result(
                "failed",
                Some(format!(
                    "同名快捷方式已存在但目标不一致{detail}；开启“覆盖同名快捷方式”后可替换"
                )),
                Some(link_path),
            );
        }
    }

    match unsafe { write_link(exe, &link_path, &name) } {
        Ok(()) => result("created", None, Some(link_path)),
        Err(error) => result("failed", Some(error), Some(link_path)),
    }
}

fn shortcut_path(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{}.lnk", sanitize_name(name)))
}

fn inspect_one_in_folder(item: &ShortcutItem, target: &str, folder: &Path) -> ShortcutState {
    let state =
        |status: &'static str, message: Option<String>, link: Option<PathBuf>| ShortcutState {
            program: item.name.clone(),
            program_path: item.path.clone(),
            target: target.to_string(),
            status,
            message,
            link_path: link.map(|path| path.to_string_lossy().into_owned()),
        };

    let exe = Path::new(&item.path);
    let link_path = shortcut_path(folder, &item.name);
    if !link_path.is_file() {
        return state("missing", None, Some(link_path));
    }

    match unsafe { read_link_target(&link_path) } {
        Some(existing) if same_target(&existing, exe) => state(
            "exists",
            Some("已存在指向该程序的快捷方式".to_string()),
            Some(link_path),
        ),
        Some(existing) => state(
            "conflict",
            Some(format!("同名快捷方式指向其他程序：{}", existing.display())),
            Some(link_path),
        ),
        None => state(
            "conflict",
            Some("同名快捷方式已存在，但无法读取它的目标路径".to_string()),
            Some(link_path),
        ),
    }
}

fn inspect_shortcuts_blocking(items: Vec<ShortcutItem>) -> Vec<ShortcutState> {
    let mut states = Vec::new();
    for item in &items {
        for target in &item.targets {
            let folder = match folder_for(target, &item.scope, &item.group) {
                Ok(folder) => folder,
                Err(error) => {
                    states.push(ShortcutState {
                        program: item.name.clone(),
                        program_path: item.path.clone(),
                        target: target.clone(),
                        status: "error",
                        message: Some(error),
                        link_path: None,
                    });
                    continue;
                }
            };
            states.push(inspect_one_in_folder(item, target, &folder));
        }
    }
    states
}

fn clear_one_in_folder(item: &ShortcutItem, folder: &Path) -> Result<bool, String> {
    let exe = Path::new(&item.path);
    let link_path = shortcut_path(folder, &item.name);
    if !link_path.is_file() {
        return Ok(false);
    }

    match unsafe { read_link_target(&link_path) } {
        Some(existing) if same_target(&existing, exe) => {
            std::fs::remove_file(&link_path)
                .map_err(|error| format!("删除快捷方式 {} 失败：{error}", link_path.display()))?;
            Ok(true)
        }
        Some(existing) => Err(format!(
            "{} 指向其他程序（{}），未删除",
            link_path.display(),
            existing.display()
        )),
        None => Err(format!(
            "{} 的目标路径无法读取，未删除",
            link_path.display()
        )),
    }
}

fn clear_shortcuts_blocking(items: Vec<ShortcutItem>, targets: Vec<String>) -> ClearResult {
    let mut result = ClearResult {
        deleted: 0,
        failed: 0,
        errors: Vec::new(),
    };

    for item in &items {
        // 新版本每个程序带自己的 targets；旧调用没有时退回外层的 targets
        let item_targets: Vec<String> = if item.targets.is_empty() {
            targets.clone()
        } else {
            item.targets.clone()
        };
        for target in &item_targets {
            let folder = match folder_for(target, &item.scope, &item.group) {
                Ok(folder) => folder,
                Err(error) => {
                    result.failed += 1;
                    result.errors.push(error);
                    continue;
                }
            };

            match clear_one_in_folder(item, &folder) {
                Ok(true) => result.deleted += 1,
                Ok(false) => {}
                Err(error) => {
                    result.failed += 1;
                    result.errors.push(error);
                }
            }

            if target == "startmenu" && !sanitize_group(&item.group).is_empty() {
                let _ = std::fs::remove_dir(&folder);
            }
        }
    }

    result
}

#[tauri::command]
async fn inspect_shortcuts(items: Vec<ShortcutItem>) -> Vec<ShortcutState> {
    tauri::async_runtime::spawn_blocking(move || inspect_shortcuts_blocking(items))
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn clear_shortcuts(items: Vec<ShortcutItem>, targets: Vec<String>) -> ClearResult {
    tauri::async_runtime::spawn_blocking(move || clear_shortcuts_blocking(items, targets))
        .await
        .unwrap_or_else(|_| ClearResult {
            deleted: 0,
            failed: 1,
            errors: vec!["清理快捷方式任务失败".to_string()],
        })
}

fn create_shortcuts_blocking(items: Vec<ShortcutItem>, overwrite: bool) -> Vec<CreateResult> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut results = Vec::new();
        for item in items {
            for target in &item.targets {
                results.push(create_one(&item, target, overwrite));
            }
        }
        if init.is_ok() {
            CoUninitialize();
        }
        results
    }
}

#[tauri::command]
async fn pick_programs(app: tauri::AppHandle) -> Result<Vec<PickedProgram>, String> {
    // HWND 不是 Send，跨线程只传原始地址，进入工作线程后再还原
    let owner_raw = app
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize);
    tauri::async_runtime::spawn_blocking(move || {
        let owner = owner_raw.map(|raw| HWND(raw as *mut c_void));
        pick_programs_blocking(owner)
    })
    .await
    .map_err(|error| format!("选择程序失败：{error}"))?
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let owner_raw = app
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize);
    tauri::async_runtime::spawn_blocking(move || {
        let owner = owner_raw.map(|raw| HWND(raw as *mut c_void));
        pick_folder_blocking(owner)
    })
    .await
    .map_err(|error| format!("选择文件夹失败：{error}"))?
}

#[tauri::command]
async fn create_shortcuts(
    items: Vec<ShortcutItem>,
    overwrite: bool,
) -> Result<Vec<CreateResult>, String> {
    let results =
        tauri::async_runtime::spawn_blocking(move || create_shortcuts_blocking(items, overwrite))
            .await
            .map_err(|error| format!("创建快捷方式失败：{error}"))?;
    Ok(results)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pid: u32,
    parent_pid: u32,
    name: String,
    path: Option<String>,
    ports: Vec<u16>,
    memory_kb: u64,
    thread_count: u32,
    protected: bool,
    protected_reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    processes: Vec<ProcessInfo>,
    current_pid: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KillResult {
    killed: usize,
    failed: usize,
    errors: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    name: String,
    value: String,
    expand: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentScopeSnapshot {
    variables: Vec<EnvironmentVariable>,
    writable: bool,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    user: EnvironmentScopeSnapshot,
    system: EnvironmentScopeSnapshot,
}

struct RawProcess {
    pid: u32,
    parent_pid: u32,
    name: String,
    thread_count: u32,
}

fn wide_slice_to_string(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|&item| item == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn registry_bytes_to_string(value: &[u8]) -> String {
    let units: Vec<u16> = value
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    wide_slice_to_string(&units)
}

fn string_to_registry_bytes(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect()
}

fn environment_key_for_scope(scope: &str) -> Result<(HKEY, &'static str), String> {
    match scope {
        "user" => Ok((HKEY_CURRENT_USER, USER_ENV_KEY)),
        "system" => Ok((HKEY_LOCAL_MACHINE, SYSTEM_ENV_KEY)),
        _ => Err(format!("不支持的环境变量范围：{scope}")),
    }
}

fn environment_scope_label(scope: &str) -> &'static str {
    if scope == "system" {
        "系统"
    } else {
        "用户"
    }
}

fn registry_error_message(scope: &str, action: &str, code: WIN32_ERROR) -> String {
    if code == ERROR_ACCESS_DENIED {
        if scope == "system" {
            format!("{action}系统环境变量失败：需要管理员权限，请以管理员身份运行本应用")
        } else {
            format!("{action}用户环境变量失败：系统拒绝访问")
        }
    } else if code == ERROR_FILE_NOT_FOUND {
        format!("{action}失败：找不到对应的环境变量或注册表项")
    } else if code == ERROR_INVALID_PARAMETER {
        format!("{action}失败：变量名或值不合法")
    } else {
        format!(
            "{action}{}环境变量失败（Windows 错误码 {}）",
            environment_scope_label(scope),
            code.0
        )
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open_environment_key(scope: &str, write: bool) -> Result<RegistryKey, String> {
    let (hive, subkey) = environment_key_for_scope(scope)?;
    let subkey = wide(subkey);
    let access = if write {
        REG_SAM_FLAGS(KEY_READ.0 | KEY_SET_VALUE.0)
    } else {
        KEY_READ
    };
    let mut key = HKEY::default();
    let code = unsafe {
        RegOpenKeyExW(
            hive,
            PCWSTR::from_raw(subkey.as_ptr()),
            None,
            access,
            &mut key,
        )
    };
    if code.0 != NO_ERROR.0 {
        return Err(registry_error_message(scope, "打开", code));
    }
    Ok(RegistryKey(key))
}

fn environment_scope_writable(scope: &str) -> bool {
    let Ok((hive, subkey)) = environment_key_for_scope(scope) else {
        return false;
    };
    let subkey = wide(subkey);
    let mut key = HKEY::default();
    let code = unsafe {
        RegOpenKeyExW(
            hive,
            PCWSTR::from_raw(subkey.as_ptr()),
            None,
            REG_SAM_FLAGS(KEY_READ.0 | KEY_SET_VALUE.0),
            &mut key,
        )
    };
    if code.0 == NO_ERROR.0 {
        unsafe {
            let _ = RegCloseKey(key);
        }
        true
    } else {
        false
    }
}

fn read_environment_values(scope: &str) -> Result<Vec<EnvironmentVariable>, String> {
    let key = open_environment_key(scope, false)?;
    let mut name_buffer = vec![0u16; 16_384];
    let mut data_buffer = vec![0u8; 65_536];
    let mut variables = Vec::new();
    let mut index = 0u32;

    loop {
        let mut name_length = name_buffer.len() as u32;
        let mut data_length = data_buffer.len() as u32;
        let mut value_type = 0u32;
        let code = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name_buffer.as_mut_ptr())),
                &mut name_length,
                None,
                Some(&mut value_type),
                Some(data_buffer.as_mut_ptr()),
                Some(&mut data_length),
            )
        };

        if code.0 == ERROR_NO_MORE_ITEMS.0 {
            break;
        }
        if code.0 == ERROR_MORE_DATA.0 {
            let next_name_length = (name_length as usize).max(name_buffer.len() * 2);
            let next_data_length = (data_length as usize).max(data_buffer.len() * 2);
            if next_name_length > 1_048_576 || next_data_length > 1_048_576 {
                return Err(format!(
                    "读取{}环境变量失败：单个变量内容过大",
                    environment_scope_label(scope)
                ));
            }
            name_buffer.resize(next_name_length, 0);
            data_buffer.resize(next_data_length, 0);
            continue;
        }
        if code.0 != NO_ERROR.0 {
            return Err(registry_error_message(scope, "读取", code));
        }

        let name = wide_slice_to_string(&name_buffer[..name_length as usize]);
        let expand = if value_type == REG_EXPAND_SZ.0 {
            true
        } else if value_type == REG_SZ.0 {
            false
        } else {
            index += 1;
            continue;
        };
        if !name.is_empty() {
            let value = registry_bytes_to_string(&data_buffer[..data_length as usize]);
            variables.push(EnvironmentVariable {
                name,
                value,
                expand,
            });
        }
        index += 1;
    }

    variables.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(variables)
}

fn environment_scope_snapshot(scope: &str) -> EnvironmentScopeSnapshot {
    let writable = environment_scope_writable(scope);
    match read_environment_values(scope) {
        Ok(variables) => EnvironmentScopeSnapshot {
            variables,
            writable,
            error: None,
        },
        Err(error) => EnvironmentScopeSnapshot {
            variables: Vec::new(),
            writable,
            error: Some(error),
        },
    }
}

fn read_environment_snapshot_blocking() -> EnvironmentSnapshot {
    EnvironmentSnapshot {
        user: environment_scope_snapshot("user"),
        system: environment_scope_snapshot("system"),
    }
}

fn environment_value_exists(key: HKEY, name: &str) -> bool {
    let name = wide(name);
    let mut data_length = 0u32;
    let code = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR::from_raw(name.as_ptr()),
            None,
            None,
            None,
            Some(&mut data_length),
        )
    };
    code.0 == NO_ERROR.0 || code.0 == ERROR_MORE_DATA.0
}

fn environment_name_eq(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

fn normalize_environment_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("变量名不能为空".to_string());
    }
    if name.contains('=') {
        return Err("变量名不能包含等号（=）".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("变量名不能包含控制字符".to_string());
    }
    Ok(name.to_string())
}

fn broadcast_environment_change() {
    let parameter = wide("Environment");
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(parameter.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            2_000,
            None,
        );
    }
}

fn save_environment_variable_blocking(
    scope: &str,
    original_name: Option<String>,
    name: String,
    value: String,
    expand: bool,
) -> Result<EnvironmentVariable, String> {
    let name = normalize_environment_name(&name)?;
    let original_name = original_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let key = open_environment_key(scope, true)?;

    if let Some(original_name) = original_name.as_deref() {
        if !environment_name_eq(original_name, &name) && environment_value_exists(key.0, &name) {
            return Err(format!("变量“{name}”已存在，请换一个名称"));
        }
    } else if environment_value_exists(key.0, &name) {
        return Err(format!("变量“{name}”已存在，请换一个名称"));
    }

    let wide_name = wide(&name);
    let data = string_to_registry_bytes(&value);
    let value_type: REG_VALUE_TYPE = if expand { REG_EXPAND_SZ } else { REG_SZ };
    let code = unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR::from_raw(wide_name.as_ptr()),
            None,
            value_type,
            Some(data.as_slice()),
        )
    };
    if code.0 != NO_ERROR.0 {
        return Err(registry_error_message(scope, "保存", code));
    }

    if let Some(original_name) = original_name {
        if !environment_name_eq(&original_name, &name) {
            let wide_original = wide(&original_name);
            let delete_code =
                unsafe { RegDeleteValueW(key.0, PCWSTR::from_raw(wide_original.as_ptr())) };
            if delete_code.0 != NO_ERROR.0 && delete_code.0 != ERROR_FILE_NOT_FOUND.0 {
                broadcast_environment_change();
                return Err(format!(
                    "新变量“{name}”已写入，但删除旧变量“{original_name}”失败：{}",
                    registry_error_message(scope, "删除", delete_code)
                ));
            }
        }
    }

    broadcast_environment_change();
    Ok(EnvironmentVariable {
        name,
        value,
        expand,
    })
}

fn delete_environment_variable_blocking(scope: &str, name: String) -> Result<(), String> {
    let name = normalize_environment_name(&name)?;
    let key = open_environment_key(scope, true)?;
    let wide_name = wide(&name);
    let code = unsafe { RegDeleteValueW(key.0, PCWSTR::from_raw(wide_name.as_ptr())) };
    if code.0 != NO_ERROR.0 {
        return Err(registry_error_message(scope, "删除", code));
    }
    broadcast_environment_change();
    Ok(())
}

fn snapshot_raw_processes() -> Result<Vec<RawProcess>, String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|error| format!("读取进程列表失败：{error}"))?;
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut processes = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = wide_slice_to_string(&entry.szExeFile);
                if !name.is_empty() {
                    processes.push(RawProcess {
                        pid: entry.th32ProcessID,
                        parent_pid: entry.th32ParentProcessID,
                        name,
                        thread_count: entry.cntThreads,
                    });
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        Ok(processes)
    }
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR::from_raw(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if result.is_err() {
            return None;
        }
        let length = (size as usize).min(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..length]))
    }
}

fn process_memory_kb(pid: u32) -> u64 {
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) {
            Ok(handle) => handle,
            Err(_) => return 0,
        };
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let result = GetProcessMemoryInfo(
            handle,
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        let _ = CloseHandle(handle);
        if result.is_err() {
            return 0;
        }
        (counters.WorkingSetSize / 1024) as u64
    }
}

fn protected_process_reason(
    pid: u32,
    name: &str,
    current_pid: u32,
    self_processes: &HashSet<u32>,
) -> Option<String> {
    if pid == current_pid {
        return Some("WinKit 工具箱自身".to_string());
    }
    if self_processes.contains(&pid) {
        return Some("WinKit 工具箱组件".to_string());
    }
    if pid == 0 {
        return Some("系统空闲进程".to_string());
    }
    if pid <= 4 {
        return Some("系统核心进程".to_string());
    }

    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "smss.exe" | "csrss.exe" | "wininit.exe" | "services.exe" | "lsass.exe" | "winlogon.exe"
    ) {
        return Some("Windows 关键进程".to_string());
    }
    if matches!(lower.as_str(), "msmpeng.exe" | "securityhealthservice.exe") {
        return Some("Windows 安全组件".to_string());
    }
    None
}

fn port_from_network(value: u32) -> u16 {
    u16::from_be((value & 0xffff) as u16)
}

fn tcp4_owner_port(row: &MIB_TCPROW_OWNER_PID) -> (u32, u16) {
    (row.dwOwningPid, port_from_network(row.dwLocalPort))
}

fn tcp6_owner_port(row: &MIB_TCP6ROW_OWNER_PID) -> (u32, u16) {
    (row.dwOwningPid, port_from_network(row.dwLocalPort))
}

fn udp4_owner_port(row: &MIB_UDPROW_OWNER_PID) -> (u32, u16) {
    (row.dwOwningPid, port_from_network(row.dwLocalPort))
}

fn udp6_owner_port(row: &MIB_UDP6ROW_OWNER_PID) -> (u32, u16) {
    (row.dwOwningPid, port_from_network(row.dwLocalPort))
}

fn collect_tcp_rows<T>(
    family: u32,
    table_class: TCP_TABLE_CLASS,
    row_info: fn(&T) -> (u32, u16),
    ports: &mut HashMap<u32, Vec<u16>>,
) {
    unsafe {
        let mut size = 0u32;
        let code = GetExtendedTcpTable(None, &mut size, false, family, table_class, 0);
        if (code != NO_ERROR.0 && code != ERROR_INSUFFICIENT_BUFFER.0) || size <= 4 {
            return;
        }
        let mut buffer = vec![0u8; size as usize];
        let code = GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            family,
            table_class,
            0,
        );
        if code != NO_ERROR.0 {
            return;
        }
        let count = std::ptr::read_unaligned(buffer.as_ptr().cast::<u32>()) as usize;
        let row_size = size_of::<T>();
        let max_rows = (size as usize).saturating_sub(size_of::<u32>()) / row_size;
        let table = buffer.as_ptr().add(size_of::<u32>()).cast::<T>();
        for index in 0..count.min(max_rows) {
            let row = std::ptr::read_unaligned(table.add(index));
            let (pid, port) = row_info(&row);
            if pid != 0 && port != 0 {
                ports.entry(pid).or_default().push(port);
            }
        }
    }
}

fn collect_udp_rows<T>(
    family: u32,
    table_class: UDP_TABLE_CLASS,
    row_info: fn(&T) -> (u32, u16),
    ports: &mut HashMap<u32, Vec<u16>>,
) {
    unsafe {
        let mut size = 0u32;
        let code = GetExtendedUdpTable(None, &mut size, false, family, table_class, 0);
        if (code != NO_ERROR.0 && code != ERROR_INSUFFICIENT_BUFFER.0) || size <= 4 {
            return;
        }
        let mut buffer = vec![0u8; size as usize];
        let code = GetExtendedUdpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            family,
            table_class,
            0,
        );
        if code != NO_ERROR.0 {
            return;
        }
        let count = std::ptr::read_unaligned(buffer.as_ptr().cast::<u32>()) as usize;
        let row_size = size_of::<T>();
        let max_rows = (size as usize).saturating_sub(size_of::<u32>()) / row_size;
        let table = buffer.as_ptr().add(size_of::<u32>()).cast::<T>();
        for index in 0..count.min(max_rows) {
            let row = std::ptr::read_unaligned(table.add(index));
            let (pid, port) = row_info(&row);
            if pid != 0 && port != 0 {
                ports.entry(pid).or_default().push(port);
            }
        }
    }
}

fn collect_process_ports() -> HashMap<u32, Vec<u16>> {
    let mut ports = HashMap::new();
    collect_tcp_rows(
        AF_INET.0 as u32,
        TCP_TABLE_OWNER_PID_LISTENER,
        tcp4_owner_port,
        &mut ports,
    );
    collect_tcp_rows(
        AF_INET6.0 as u32,
        TCP_TABLE_OWNER_PID_LISTENER,
        tcp6_owner_port,
        &mut ports,
    );
    collect_udp_rows(
        AF_INET.0 as u32,
        UDP_TABLE_OWNER_PID,
        udp4_owner_port,
        &mut ports,
    );
    collect_udp_rows(
        AF_INET6.0 as u32,
        UDP_TABLE_OWNER_PID,
        udp6_owner_port,
        &mut ports,
    );
    for values in ports.values_mut() {
        values.sort_unstable();
        values.dedup();
    }
    ports
}

fn list_processes_blocking() -> Result<ProcessSnapshot, String> {
    let current_pid = std::process::id();
    let raw = snapshot_raw_processes()?;
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in &raw {
        children
            .entry(process.parent_pid)
            .or_default()
            .push(process.pid);
    }
    let self_processes = process_tree_ids(current_pid, &children);
    let process_ports = collect_process_ports();

    let mut processes = raw
        .into_iter()
        .map(|process| {
            let protected_reason =
                protected_process_reason(process.pid, &process.name, current_pid, &self_processes);
            ProcessInfo {
                pid: process.pid,
                parent_pid: process.parent_pid,
                path: process_path(process.pid),
                ports: process_ports.get(&process.pid).cloned().unwrap_or_default(),
                memory_kb: process_memory_kb(process.pid),
                thread_count: process.thread_count,
                protected: protected_reason.is_some(),
                protected_reason,
                name: process.name,
            }
        })
        .collect::<Vec<_>>();

    processes.sort_by(|left, right| {
        left.protected
            .cmp(&right.protected)
            .then_with(|| right.memory_kb.cmp(&left.memory_kb))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.pid.cmp(&right.pid))
    });

    Ok(ProcessSnapshot {
        processes,
        current_pid,
    })
}

fn collect_process_tree(
    pid: u32,
    children: &HashMap<u32, Vec<u32>>,
    seen: &mut HashSet<u32>,
    order: &mut Vec<u32>,
) {
    if !seen.insert(pid) {
        return;
    }
    if let Some(list) = children.get(&pid) {
        for child in list {
            collect_process_tree(*child, children, seen, order);
        }
    }
    order.push(pid);
}

fn process_tree_ids(root: u32, children: &HashMap<u32, Vec<u32>>) -> HashSet<u32> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    collect_process_tree(root, children, &mut seen, &mut order);
    order.into_iter().collect()
}

fn terminate_process(pid: u32) -> Result<(), String> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|error| format!("无法打开进程：{error}"))?;
        let result = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        result.map_err(|error| format!("结束失败：{error}"))
    }
}

fn kill_processes_blocking(pids: Vec<u32>, include_tree: bool) -> Result<KillResult, String> {
    let current_pid = std::process::id();
    let raw = snapshot_raw_processes()?;
    let names = raw
        .iter()
        .map(|process| (process.pid, process.name.clone()))
        .collect::<HashMap<_, _>>();

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in &raw {
        children
            .entry(process.parent_pid)
            .or_default()
            .push(process.pid);
    }
    let self_processes = process_tree_ids(current_pid, &children);

    let requested = pids.into_iter().collect::<HashSet<_>>();
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let mut result = KillResult {
        killed: 0,
        failed: 0,
        errors: Vec::new(),
    };

    for pid in requested {
        let name = match names.get(&pid) {
            Some(name) => name,
            None => {
                result.failed += 1;
                result.errors.push(format!("PID {pid} 已经不存在"));
                continue;
            }
        };

        if let Some(reason) = protected_process_reason(pid, name, current_pid, &self_processes) {
            result.failed += 1;
            result
                .errors
                .push(format!("{name} (PID {pid}) 受保护：{reason}"));
            continue;
        }

        if include_tree {
            collect_process_tree(pid, &children, &mut seen, &mut targets);
        } else if seen.insert(pid) {
            targets.push(pid);
        }
    }

    for pid in targets {
        let name = names
            .get(&pid)
            .cloned()
            .unwrap_or_else(|| "未知进程".to_string());
        if let Some(reason) = protected_process_reason(pid, &name, current_pid, &self_processes) {
            result.failed += 1;
            result
                .errors
                .push(format!("{name} (PID {pid}) 受保护：{reason}"));
            continue;
        }

        match terminate_process(pid) {
            Ok(()) => result.killed += 1,
            Err(error) => {
                result.failed += 1;
                result.errors.push(format!("{name} (PID {pid})：{error}"));
            }
        }
    }

    Ok(result)
}

#[tauri::command]
async fn list_processes() -> Result<ProcessSnapshot, String> {
    tauri::async_runtime::spawn_blocking(list_processes_blocking)
        .await
        .map_err(|error| format!("读取进程列表失败：{error}"))?
}

#[tauri::command]
async fn kill_processes(pids: Vec<u32>, include_tree: bool) -> Result<KillResult, String> {
    tauri::async_runtime::spawn_blocking(move || kill_processes_blocking(pids, include_tree))
        .await
        .map_err(|error| format!("结束进程任务失败：{error}"))?
}

#[tauri::command]
async fn load_environment_variables() -> Result<EnvironmentSnapshot, String> {
    tauri::async_runtime::spawn_blocking(read_environment_snapshot_blocking)
        .await
        .map_err(|error| format!("读取环境变量失败：{error}"))
}

#[tauri::command]
async fn save_environment_variable(
    scope: String,
    original_name: Option<String>,
    name: String,
    value: String,
    expand: bool,
) -> Result<EnvironmentVariable, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_environment_variable_blocking(&scope, original_name, name, value, expand)
    })
    .await
    .map_err(|error| format!("保存环境变量失败：{error}"))?
}

#[tauri::command]
async fn delete_environment_variable(scope: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_environment_variable_blocking(&scope, name))
        .await
        .map_err(|error| format!("删除环境变量失败：{error}"))?
}

#[cfg(windows)]
fn enforce_single_instance() {
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let mutex_name = wide(SINGLE_INSTANCE_MUTEX);
        let handle = match CreateMutexW(None, true, PCWSTR::from_raw(mutex_name.as_ptr())) {
            Ok(handle) => handle,
            Err(_) => return,
        };
        // HANDLE 没有 Drop 实现，进程退出时由系统回收
        let _handle = handle;

        if GetLastError() != ERROR_ALREADY_EXISTS {
            return;
        }

        let title = wide(WINDOW_TITLE);
        if let Ok(window) = FindWindowW(None, PCWSTR::from_raw(title.as_ptr())) {
            if !window.0.is_null() {
                let _ = ShowWindow(window, SW_RESTORE);
                let _ = SetForegroundWindow(window);
            }
        }
        std::process::exit(0);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    enforce_single_instance();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            pick_programs,
            pick_folder,
            create_shortcuts,
            load_library,
            save_library,
            program_icons,
            inspect_shortcuts,
            clear_shortcuts,
            list_processes,
            kill_processes,
            load_environment_variables,
            save_environment_variable,
            delete_environment_variable,
            load_environment_profiles,
            save_environment_profiles,
            service_manager::load_service_configs,
            service_manager::save_service_configs,
            service_manager::register_service_command,
            service_manager::unregister_service_command,
            service_manager::start_service_command,
            service_manager::stop_service_command,
            service_manager::restart_service_command,
            service_manager::query_service_status_command,
            service_manager::apply_all_services,
            service_manager::capture_runtime_env
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_shortcut_into_temp_folder() {
        let exe = PathBuf::from(r"C:\Windows\System32\notepad.exe");
        assert!(exe.is_file(), "notepad.exe 应存在于系统中");

        let folder = std::env::temp_dir().join("appshortcut-lnk-test");
        std::fs::create_dir_all(&folder).expect("创建测试目录");
        let link_path = folder.join("Notepad 测试.lnk");
        let _ = std::fs::remove_file(&link_path);

        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            write_link(&exe, &link_path, "Notepad 测试").expect("写入快捷方式");
            if init.is_ok() {
                CoUninitialize();
            }
        }

        assert!(link_path.is_file(), "快捷方式文件应已生成");
        assert!(std::fs::metadata(&link_path).expect("读取元数据").len() > 0);

        let _ = std::fs::remove_file(&link_path);
        let _ = std::fs::remove_dir(&folder);
    }

    #[test]
    fn sanitizes_invalid_file_name_characters() {
        assert_eq!(sanitize_name("a/b:c*?\"<>|d"), "a_b_c______d");
        assert_eq!(sanitize_name("   "), "快捷方式");
    }

    #[test]
    fn skips_when_existing_shortcut_name_and_target_match() {
        let exe = PathBuf::from(r"C:\Windows\System32\notepad.exe");
        assert!(exe.is_file(), "notepad.exe 应存在于系统中");

        let folder = std::env::temp_dir().join("appshortcut-skip-test");
        std::fs::create_dir_all(&folder).expect("创建测试目录");
        let link_path = folder.join("Skip 测试.lnk");
        let _ = std::fs::remove_file(&link_path);

        let item = ShortcutItem {
            name: "Skip 测试".to_string(),
            path: exe.to_string_lossy().into_owned(),
            targets: vec!["desktop".to_string()],
            scope: "current".to_string(),
            group: "RustRover家族".to_string(),
        };

        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            write_link(&exe, &link_path, "Skip 测试").expect("写入已有快捷方式");
            if init.is_ok() {
                CoUninitialize();
            }
        }

        let before = std::fs::read(&link_path).expect("读取已有快捷方式");
        let skipped = create_one_in_folder(&item, "desktop", &folder, false);
        assert_eq!(skipped.status, "skipped");
        assert_eq!(
            std::fs::read(&link_path).expect("再次读取"),
            before,
            "同名且目标一致时不应改动已有文件"
        );

        let _ = std::fs::remove_file(&link_path);
        let _ = std::fs::remove_dir(&folder);
    }

    #[test]
    fn rejects_same_name_with_different_target_unless_overwrite() {
        let exe = PathBuf::from(r"C:\Windows\System32\notepad.exe");
        let other = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        assert!(exe.is_file() && other.is_file());

        let folder = std::env::temp_dir().join("appshortcut-conflict-test");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("创建测试目录");
        let link_path = folder.join("Conflict 测试.lnk");

        let item = ShortcutItem {
            name: "Conflict 测试".to_string(),
            path: exe.to_string_lossy().into_owned(),
            targets: vec!["desktop".to_string()],
            scope: "current".to_string(),
            group: String::new(),
        };

        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            write_link(&other, &link_path, "Conflict 测试").expect("写入指向 cmd 的快捷方式");
            if init.is_ok() {
                CoUninitialize();
            }
        }

        let before = std::fs::read(&link_path).expect("读取已有快捷方式");
        let rejected = create_one_in_folder(&item, "desktop", &folder, false);
        assert_eq!(rejected.status, "failed");
        assert!(
            rejected
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("目标不一致")
        );
        assert_eq!(std::fs::read(&link_path).expect("再次读取"), before);

        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let created = create_one_in_folder(&item, "desktop", &folder, true);
            if init.is_ok() {
                CoUninitialize();
            }
            assert_eq!(created.status, "created");
        }

        let target = unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let target = read_link_target(&link_path);
            if init.is_ok() {
                CoUninitialize();
            }
            target
        };
        assert!(
            target
                .as_deref()
                .map(|value| same_target(value, &exe))
                .unwrap_or(false),
            "覆盖后应指向 notepad"
        );

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn start_menu_shortcuts_use_custom_group_folder() {
        let folder =
            folder_for("startmenu", "current", "RustRover家族").expect("读取当前用户开始菜单");
        assert_eq!(
            folder.file_name().and_then(|name| name.to_str()),
            Some("RustRover家族")
        );
        let desktop = folder_for("desktop", "current", "RustRover家族").expect("读取桌面目录");
        assert_ne!(
            desktop.file_name().and_then(|name| name.to_str()),
            Some("RustRover家族")
        );
        let root = folder_for("startmenu", "current", "").expect("读取开始菜单根目录");
        assert_ne!(
            root.file_name().and_then(|name| name.to_str()),
            Some("RustRover家族")
        );
    }

    #[test]
    fn status_and_clear_use_actual_link_target() {
        let exe = PathBuf::from(r"C:\Windows\System32\notepad.exe");
        let other = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        assert!(exe.is_file() && other.is_file());

        let folder = std::env::temp_dir().join("appshortcut-status-test");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("创建测试目录");
        let link_path = folder.join("Status 测试.lnk");

        let item = ShortcutItem {
            name: "Status 测试".to_string(),
            path: exe.to_string_lossy().into_owned(),
            targets: vec!["desktop".to_string()],
            scope: "current".to_string(),
            group: String::new(),
        };

        let missing = inspect_one_in_folder(&item, "desktop", &folder);
        assert_eq!(missing.status, "missing");

        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            write_link(&other, &link_path, "Status 测试").expect("写入指向 cmd 的快捷方式");
            if init.is_ok() {
                CoUninitialize();
            }
        }

        let conflict = inspect_one_in_folder(&item, "desktop", &folder);
        assert_eq!(conflict.status, "conflict");
        assert!(
            conflict
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("其他程序")
        );
        assert!(
            clear_one_in_folder(&item, &folder).is_err(),
            "冲突文件不应删除"
        );
        assert!(link_path.is_file());

        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            write_link(&exe, &link_path, "Status 测试").expect("改写成指向 notepad 的快捷方式");
            if init.is_ok() {
                CoUninitialize();
            }
        }

        let exists = inspect_one_in_folder(&item, "desktop", &folder);
        assert_eq!(exists.status, "exists");
        assert!(clear_one_in_folder(&item, &folder).expect("删除匹配快捷方式"));
        assert!(!link_path.is_file(), "清除标记应删除真实快捷方式");

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn extracts_program_icon_as_png_data_url() {
        let icon = unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let icon = icon_for_path(r"C:\Windows\System32\notepad.exe");
            if init.is_ok() {
                CoUninitialize();
            }
            icon
        };
        let value = icon.expect("notepad 应能取到图标");
        assert!(value.starts_with("data:image/png;base64,"));
        assert!(value.len() > 128, "图标数据不应为空");
    }

    #[test]
    fn library_file_round_trip_and_quarantines_invalid_json() {
        let folder = std::env::temp_dir().join("appshortcut-library-test");
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join(LIBRARY_FILE_NAME);

        let text = r#"{"version":1,"programs":[]}"#;
        write_library_at(&path, text).expect("写入清单");
        assert_eq!(
            read_library_at(&path).expect("读取清单").as_deref(),
            Some(text)
        );
        assert!(!folder.join(format!("{LIBRARY_FILE_NAME}.tmp")).exists());

        std::fs::write(&path, b"not json").expect("写入坏数据");
        assert!(read_library_at(&path).expect("读取坏数据").is_none());
        assert!(!path.exists(), "损坏的清单应被移走");
        let backup = std::fs::read_dir(&folder)
            .expect("读取目录")
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.file_name().to_string_lossy().contains("invalid-"));
        assert!(backup.is_some(), "应保留损坏清单的备份");

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn migrates_legacy_library_file_to_winkit_name() {
        let folder =
            std::env::temp_dir().join(format!("winkit-library-migration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let legacy = folder.join(LEGACY_LIBRARY_FILE_NAME);
        let target = folder.join(LIBRARY_FILE_NAME);
        let text = r#"{"version":1,"programs":[]}"#;

        write_library_at(&legacy, text).expect("写入旧清单");
        let content = read_and_migrate_legacy_library(&legacy, &target)
            .expect("迁移旧清单")
            .expect("旧清单应有内容");
        assert_eq!(content, text);
        assert!(target.exists(), "旧清单应改名为 WinKit.json");
        assert!(!legacy.exists(), "旧 shortcuts.json 不应保留");

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn protects_core_and_self_processes() {
        let current = std::process::id();
        let empty = HashSet::new();
        assert!(protected_process_reason(current, "WinKit.exe", current, &empty).is_some());
        assert!(protected_process_reason(0, "System Idle Process", current, &empty).is_some());
        assert!(protected_process_reason(4, "System", current, &empty).is_some());
        assert!(protected_process_reason(100, "lsass.exe", current, &empty).is_some());
        assert!(protected_process_reason(100, "node.exe", current, &empty).is_none());
        assert!(protected_process_reason(100, "msedgewebview2.exe", current, &empty).is_none());

        let mut self_tree = HashSet::new();
        self_tree.insert(100);
        let component = protected_process_reason(100, "msedgewebview2.exe", current, &self_tree)
            .expect("自身组件应受保护");
        assert!(component.contains("WinKit 工具箱"));
    }

    #[test]
    fn process_snapshot_contains_current_process() {
        let snapshot = list_processes_blocking().expect("读取进程快照");
        assert!(!snapshot.processes.is_empty());
        let current = snapshot
            .processes
            .iter()
            .find(|process| process.pid == snapshot.current_pid)
            .expect("快照里应包含当前进程");
        assert!(current.protected);
        assert!(
            current
                .protected_reason
                .as_deref()
                .unwrap_or_default()
                .contains("WinKit 工具箱")
        );
    }

    #[test]
    fn decodes_network_order_port() {
        assert_eq!(port_from_network(0x0000_901F), 8080);
        assert_eq!(port_from_network(0x0000_BB01), 443);
        assert_eq!(port_from_network(0x0000_5000), 80);
    }

    #[test]
    fn maps_listening_tcp_port_to_current_process() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定测试端口");
        let port = listener.local_addr().expect("读取测试端口").port();
        let ports = collect_process_ports();
        let current = ports.get(&std::process::id()).cloned().unwrap_or_default();
        assert!(
            current.contains(&port),
            "进程端口映射应包含测试监听端口 {port}，实际为 {current:?}"
        );
    }

    #[test]
    fn reads_user_environment_values() {
        let snapshot = read_environment_snapshot_blocking();
        assert!(
            snapshot.user.error.is_none(),
            "用户环境变量应能正常读取：{:?}",
            snapshot.user.error
        );
        assert!(
            !snapshot.user.variables.is_empty(),
            "当前用户环境变量不应为空"
        );
    }

    struct TestEnvironmentVariable {
        name: String,
    }

    impl Drop for TestEnvironmentVariable {
        fn drop(&mut self) {
            let _ = delete_environment_variable_blocking("user", self.name.clone());
        }
    }

    #[test]
    fn saves_and_deletes_user_environment_value() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("读取系统时间")
            .as_millis();
        let name = format!("APPSHORTCUT_TEST_ENV_{}_{}", std::process::id(), stamp);
        let value = r"%USERPROFILE%\AppShortcutTest".to_string();
        let _guard = TestEnvironmentVariable { name: name.clone() };

        let saved =
            save_environment_variable_blocking("user", None, name.clone(), value.clone(), true)
                .expect("写入测试环境变量");
        assert_eq!(saved.name, name);
        assert_eq!(saved.value, value);
        assert!(saved.expand);

        let snapshot = read_environment_snapshot_blocking();
        let found = snapshot
            .user
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .expect("读回刚写入的环境变量");
        assert_eq!(found.value, value);
        assert!(found.expand);

        delete_environment_variable_blocking("user", name.clone()).expect("删除测试环境变量");
        let after = read_environment_snapshot_blocking();
        assert!(
            !after
                .user
                .variables
                .iter()
                .any(|variable| variable.name == name),
            "删除后不应再读到测试环境变量"
        );
    }

    #[test]
    fn renames_user_environment_value_and_removes_old_name() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("读取系统时间")
            .as_millis();
        let original = format!("APPSHORTCUT_TEST_ENV_OLD_{}_{}", std::process::id(), stamp);
        let renamed = format!("{original}_NEW");
        let _old_guard = TestEnvironmentVariable {
            name: original.clone(),
        };
        let _new_guard = TestEnvironmentVariable {
            name: renamed.clone(),
        };

        save_environment_variable_blocking(
            "user",
            None,
            original.clone(),
            "old-value".to_string(),
            false,
        )
        .expect("写入旧变量");
        save_environment_variable_blocking(
            "user",
            Some(original.clone()),
            renamed.clone(),
            "%USERPROFILE%\\Renamed".to_string(),
            true,
        )
        .expect("改名环境变量");

        let snapshot = read_environment_snapshot_blocking();
        assert!(
            !snapshot
                .user
                .variables
                .iter()
                .any(|variable| variable.name == original),
            "改名后旧变量不应存在"
        );
        let found = snapshot
            .user
            .variables
            .iter()
            .find(|variable| variable.name == renamed)
            .expect("改名后的变量应存在");
        assert_eq!(found.value, r"%USERPROFILE%\Renamed");
        assert!(found.expand);

        delete_environment_variable_blocking("user", renamed).expect("删除改名后的变量");
    }

    #[test]
    fn rejects_invalid_environment_name() {
        assert!(normalize_environment_name("   ").is_err());
        assert!(normalize_environment_name("BAD=NAME").is_err());
        assert!(normalize_environment_name("  GOOD_NAME  ").is_ok());
    }

    #[test]
    fn environment_profile_file_round_trip_and_quarantines_invalid_json() {
        let folder = std::env::temp_dir().join(format!(
            "appshortcut-env-profile-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("读取系统时间")
                .as_millis()
        ));
        std::fs::create_dir_all(&folder).expect("创建测试目录");
        let path = folder.join("environment-profiles.json");
        let content = r#"{"version":1,"profiles":[{"id":"java","name":"JAVA","variables":[]}]}"#;

        write_environment_profile_at(&path, content).expect("写入方案文件");
        assert_eq!(
            read_environment_profile_at(&path).expect("读取方案文件"),
            Some(content.to_string())
        );

        std::fs::write(&path, "{not-json").expect("写入损坏 JSON");
        assert!(
            read_environment_profile_at(&path)
                .expect("损坏文件应被隔离")
                .is_none()
        );
        assert!(
            std::fs::read_dir(&folder)
                .expect("列目录")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains("invalid-")),
            "损坏文件应改名为 invalid-* 备份"
        );

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn empty_kill_request_is_a_noop() {
        let result = kill_processes_blocking(Vec::new(), false).expect("读取进程快照");
        assert_eq!(result.killed, 0);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());
    }
}
