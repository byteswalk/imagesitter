//! 捕获命令：绑定目标窗口、启动捕获流、取帧 PNG 预览与逐点采样。

use base64::Engine;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

use crate::state::{AppState, TargetSession};
use crate::winsitter::{
    sdk, WindowRestoreOptions, WindowSessionOptions, WINDOW_STATE_MINIMIZED,
    WINSITTER_ERR_CAPTURE_WINDOW_MINIMIZED, WINSITTER_ERR_WINDOW_ACCESS_DENIED, WINSITTER_OK,
};

use super::grab_frame;

/// 绑定结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundTarget {
    /// ImageSitter 目标 ID。
    pub target_id: u64,
    /// 绑定的 HWND。
    pub hwnd: usize,
    /// 窗口标题。
    pub title: String,
    /// 自动恢复最小化窗口失败时的提示；成功或无需恢复时为 None。
    pub restore_warning: Option<String>,
}

/// PNG 帧载荷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePayload {
    /// data URL 形式的 PNG（base64）。
    pub png_data_url: String,
    /// 帧宽度（像素）。
    pub width: u32,
    /// 帧高度（像素）。
    pub height: u32,
}

/// 采样点坐标。
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplePoint {
    /// 横坐标（帧坐标系）。
    pub x: u32,
    /// 纵坐标（帧坐标系）。
    pub y: u32,
}

/// 绑定一个窗口并启动后台捕获流；窗口处于最小化时先恢复到原位置（不抢前景、保持 Z 序）。
#[tauri::command]
pub fn bind_target(
    state: State<AppState>,
    hwnd: usize,
    title: String,
    window_state: u32,
) -> Result<BoundTarget, String> {
    if hwnd == 0 {
        return Err("E1101: 无效的窗口句柄".to_string());
    }
    let options = WindowSessionOptions::default();
    let winsitter = sdk()?;
    let session_id = winsitter.bind_window(hwnd, &options).map_err(|code| {
        format!("E1102: 绑定窗口失败（winsitter 错误码 {code}），窗口可能已关闭")
    })?;

    // 最小化窗口先恢复再启动捕获，避免拿到空帧；恢复失败不阻断绑定
    let mut restore_warning = None;
    if window_state == WINDOW_STATE_MINIMIZED {
        match winsitter.window_restore(session_id, &WindowRestoreOptions::default()) {
            Ok(_) => {}
            Err(WINSITTER_ERR_WINDOW_ACCESS_DENIED) => {
                restore_warning = Some(
                    "窗口已最小化，恢复被 Windows 以权限不足拒绝；可在左侧窗口列表重新点击该窗口，会自动弹 UAC 提权"
                        .to_string(),
                );
            }
            Err(code) => {
                restore_warning = Some(format!(
                    "窗口已最小化且自动恢复失败（winsitter 错误码 {code}）；可在左侧窗口列表重新点击该窗口触发恢复"
                ));
            }
        }
    }

    let code = winsitter.capture_start(session_id);
    if code == WINSITTER_ERR_CAPTURE_WINDOW_MINIMIZED {
        winsitter.release_window(session_id);
        return Err("E1103: 目标窗口已最小化，无法产生有效画面，请先恢复窗口再绑定".to_string());
    }
    if code != WINSITTER_OK {
        winsitter.release_window(session_id);
        return Err(format!("E1103: 启动捕获流失败（winsitter 错误码 {code}）"));
    }

    let mut next_id = state
        .next_target_id
        .lock()
        .map_err(|_| "E9000: 状态锁中毒".to_string())?;
    *next_id += 1;
    let target_id = *next_id;
    drop(next_id);

    let mut targets = state
        .targets
        .lock()
        .map_err(|_| "E9000: 状态锁中毒".to_string())?;
    targets.insert(
        target_id,
        Arc::new(Mutex::new(TargetSession {
            target_id,
            session_id,
            hwnd,
            title: title.clone(),
        })),
    );
    Ok(BoundTarget {
        target_id,
        hwnd,
        title,
        restore_warning,
    })
}

/// 释放一个已绑定的目标窗口。
#[tauri::command]
pub fn unbind_target(state: State<AppState>, target_id: u64) -> Result<(), String> {
    let session = {
        let mut targets = state
            .targets
            .lock()
            .map_err(|_| "E9000: 状态锁中毒".to_string())?;
        targets.remove(&target_id)
    };
    if let Some(session) = session {
        let session = session
            .lock()
            .map_err(|_| "E9000: 会话锁中毒".to_string())?;
        sdk()?.release_window(session.session_id);
        Ok(())
    } else {
        Err("E2001: 目标未绑定或已释放".to_string())
    }
}

/// 点击窗口列表项时立即恢复最小化窗口：临时会话 绑定→恢复→释放（全程 winsitter 接口，
/// 不抢前景、保持原 Z 序）。
#[tauri::command]
pub fn restore_window(hwnd: usize) -> Result<(), String> {
    if hwnd == 0 {
        return Err("E1101: 无效的窗口句柄".to_string());
    }
    let winsitter = sdk()?;
    let session_id = winsitter
        .bind_window(hwnd, &WindowSessionOptions::default())
        .map_err(|code| {
            format!("E1102: 绑定窗口失败（winsitter 错误码 {code}），窗口可能已关闭")
        })?;
    let outcome = winsitter.window_restore(session_id, &WindowRestoreOptions::default());
    winsitter.release_window(session_id);
    outcome.map(|_| ()).map_err(|code| {
        if code == WINSITTER_ERR_WINDOW_ACCESS_DENIED {
            "E1104: 权限不足，恢复被 Windows 拒绝（winsitter 错误码 -1507）".to_string()
        } else {
            format!("E1104: 恢复窗口失败（winsitter 错误码 {code}）")
        }
    })
}

/// 当前进程是否已以管理员（提升）权限运行。
fn is_process_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// 把字符串编码为以 0 结尾的 UTF-16，供 Win32 W 系列 API 使用。
fn to_wide(text: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::path::Path::new(text)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 动态提权：弹 UAC 以管理员重启本程序；当前实例随后退出。已是管理员时返回错误。
#[tauri::command]
pub fn relaunch_elevated() -> Result<(), String> {
    if is_process_elevated() {
        return Err("E1105: 本工具已以管理员权限运行，无需提权".to_string());
    }
    let exe = std::env::current_exe().map_err(|error| format!("E1106: {error}"))?;
    let dir = exe
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    let verb = to_wide("runas");
    let path = to_wide(&exe.display().to_string());
    let dir_w = to_wide(&dir);
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<HWND>() as _,
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            dir_w.as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        return Err(format!(
            "E1107: 提权启动失败（ShellExecute 代码 {}），可能已取消 UAC",
            result as isize
        ));
    }
    // 给 IPC 响应留出到达前端的时间，然后退出当前实例
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
    Ok(())
}

/// 目标窗口客户区原点的屏幕坐标，供标尺把帧坐标换算为屏幕位置。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenOrigin {
    /// 客户区左上角在屏幕上的 X。
    pub x: i32,
    /// 客户区左上角在屏幕上的 Y。
    pub y: i32,
}

/// 查询目标窗口客户区原点在屏幕上的绝对位置（帧 0,0 对应的屏幕点）。
#[tauri::command]
pub fn target_screen_origin(
    state: State<AppState>,
    target_id: u64,
) -> Result<ScreenOrigin, String> {
    let shared = super::lookup_session(&state, target_id)?;
    let hwnd = shared
        .lock()
        .map_err(|_| "E9000: 会话锁中毒".to_string())?
        .hwnd;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    let mut point = POINT { x: 0, y: 0 };
    let ok = unsafe { ClientToScreen(hwnd as _, &mut point) };
    if ok == 0 {
        return Err("E2201: 无法换算窗口屏幕坐标".to_string());
    }
    Ok(ScreenOrigin {
        x: point.x,
        y: point.y,
    })
}

/// 抓取目标窗口最新整帧并编码为 PNG data URL，供前端预览。
#[tauri::command]
pub fn capture_frame_png(state: State<AppState>, target_id: u64) -> Result<FramePayload, String> {
    let (rgba, width, height) = grab_frame(&state, target_id)?;
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "E2104: 帧缓冲尺寸不一致".to_string())?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| format!("E2105: PNG 编码失败：{error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(FramePayload {
        png_data_url: format!("data:image/png;base64,{encoded}"),
        width,
        height,
    })
}

/// 在最新帧上采样多个点的 RGBA 颜色；越界点返回 null。
#[tauri::command]
pub fn sample_points(
    state: State<AppState>,
    target_id: u64,
    points: Vec<SamplePoint>,
) -> Result<Vec<Option<[u8; 4]>>, String> {
    let (frame, width, height) = grab_frame(&state, target_id)?;
    Ok(points
        .into_iter()
        .map(|point| {
            if point.x >= width || point.y >= height {
                return None;
            }
            let offset = ((point.y * width + point.x) * 4) as usize;
            Some([
                frame[offset],
                frame[offset + 1],
                frame[offset + 2],
                frame[offset + 3],
            ])
        })
        .collect())
}
