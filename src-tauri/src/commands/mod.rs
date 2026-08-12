//! Tauri IPC 命令：窗口发现、捕获取帧、特征匹配与项目文件读写。

pub mod calibrate;
pub mod capture;
pub mod matching;
pub mod project;
pub mod windows;

use crate::state::AppState;
use crate::state::TargetSession;
use crate::winsitter::{sdk, Symbols, WINSITTER_ERR_BUFFER_TOO_SMALL, WINSITTER_OK};
use std::sync::{Arc, Mutex};

/// 从状态表取出目标会话的关键信息；避免长时间持有锁。
fn lookup_session(state: &AppState, target_id: u64) -> Result<Arc<Mutex<TargetSession>>, String> {
    let targets = state
        .targets
        .lock()
        .map_err(|_| "E9000: 状态锁中毒".to_string())?;
    targets
        .get(&target_id)
        .cloned()
        .ok_or_else(|| "E2001: 目标未绑定或已释放".to_string())
}

/// 计算 WGC 整帧内的客户区裁剪矩形（帧坐标系）；无需裁剪时返回 None。
///
/// winsitter 窗口捕获基于 WGC，帧覆盖整个窗口视觉范围（含可见边框，
/// 与扩展帧边界一致）；客户区原点经 Win32 `ClientToScreen` 相对帧边界换算。
fn client_crop(
    winsitter: &Symbols,
    session_id: u64,
    hwnd: usize,
    frame_w: u32,
    frame_h: u32,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    let info = winsitter
        .window_get_info(session_id)
        .map_err(|code| format!("E2202: 无法确认捕获帧坐标系（winsitter 错误码 {code}）"))?;
    if info.client_width == 0 || info.client_height == 0 {
        return Err("E2203: 目标窗口客户区尺寸无效，已停止取帧以避免坐标漂移".to_string());
    }
    // 某些捕获后端直接输出客户区，此时无需换算裁剪偏移。
    if info.client_width == frame_w && info.client_height == frame_h {
        return Ok(None);
    }
    // 仅当帧尺寸与 winsitter 报告的扩展窗口帧一致时才信任原点换算。
    let expected_w = (i64::from(info.right) - i64::from(info.left)).max(0) as u32;
    let expected_h = (i64::from(info.bottom) - i64::from(info.top)).max(0) as u32;
    if expected_w != frame_w || expected_h != frame_h {
        return Err(format!(
            "E2204: 捕获帧 {}x{} 与窗口边界 {}x{}、客户区 {}x{} 不一致，已停止取帧以避免使用错误坐标",
            frame_w, frame_h, expected_w, expected_h, info.client_width, info.client_height
        ));
    }
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { ClientToScreen(hwnd as _, &mut point) } == 0 {
        return Err("E2205: 无法换算客户区原点，已停止取帧以避免坐标漂移".to_string());
    }
    let ox = i64::from(point.x) - i64::from(info.left);
    let oy = i64::from(point.y) - i64::from(info.top);
    if ox < 0 || oy < 0 {
        return Err(format!(
            "E2206: 客户区原点偏移异常 ({ox}, {oy})，已停止取帧以避免坐标漂移"
        ));
    }
    let (ox, oy) = (ox as u32, oy as u32);
    let (cw, ch) = (info.client_width, info.client_height);
    if ox.checked_add(cw).is_none_or(|right| right > frame_w)
        || oy.checked_add(ch).is_none_or(|bottom| bottom > frame_h)
    {
        return Err(format!(
            "E2207: 客户区裁剪范围 ({ox}, {oy}, {cw}, {ch}) 超出捕获帧 {frame_w}x{frame_h}"
        ));
    }
    if ox == 0 && oy == 0 && cw == frame_w && ch == frame_h {
        return Ok(None);
    }
    Ok(Some((ox, oy, cw, ch)))
}

/// 从 RGBA8 整帧缓冲按矩形裁剪出子图（逐行拷贝）。
fn crop_rgba(buffer: &[u8], src_w: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let src_stride = src_w as usize * 4;
    let row_bytes = w as usize * 4;
    let mut out = Vec::with_capacity(row_bytes * h as usize);
    for row in 0..h as usize {
        let start = (y as usize + row) * src_stride + x as usize * 4;
        out.extend_from_slice(&buffer[start..start + row_bytes]);
    }
    out
}

/// 抓取目标窗口最新客户区帧 RGBA8 像素（已裁掉窗口边框）。
fn grab_frame(state: &AppState, target_id: u64) -> Result<(Vec<u8>, u32, u32), String> {
    let shared = lookup_session(state, target_id)?;
    // 与 unbind 串行化，避免 DLL 会话在取帧途中被另一个 IPC 释放。
    let session = shared.lock().map_err(|_| "E9000: 会话锁中毒".to_string())?;
    let session_id = session.session_id;
    let hwnd = session.hwnd;
    let winsitter = sdk()?;
    let mut width = 0i32;
    let mut height = 0i32;
    let mut size = 0i32;
    // 先用空缓冲查询帧尺寸
    let code = winsitter.capture_rgba_into(session_id, &mut [], &mut width, &mut height, &mut size);
    if code != WINSITTER_OK && code != WINSITTER_ERR_BUFFER_TOO_SMALL {
        return Err(format!(
            "E2101: 取帧失败（winsitter 错误码 {code}），窗口可能已关闭或最小化"
        ));
    }
    if size <= 0 || width <= 0 || height <= 0 {
        return Err("E2102: 捕获流尚未产生有效帧，请确认目标窗口可见".to_string());
    }
    let mut buffer = vec![0u8; size as usize];
    let mut code =
        winsitter.capture_rgba_into(session_id, &mut buffer, &mut width, &mut height, &mut size);
    // 窗口恰在两次调用之间改变尺寸时，按 SDK 回写的新容量重试一次。
    if code == WINSITTER_ERR_BUFFER_TOO_SMALL && size > buffer.len() as i32 {
        buffer.resize(size as usize, 0);
        code = winsitter.capture_rgba_into(
            session_id,
            &mut buffer,
            &mut width,
            &mut height,
            &mut size,
        );
    }
    if code != WINSITTER_OK {
        return Err(format!("E2103: 读取帧像素失败（winsitter 错误码 {code}）"));
    }
    buffer.truncate(size.max(0) as usize);
    let (frame_w, frame_h) = (width as u32, height as u32);
    let expected_len = u64::from(frame_w)
        .checked_mul(u64::from(frame_h))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "E2104: 捕获帧尺寸计算溢出".to_string())?;
    if expected_len != buffer.len() as u64 {
        return Err(format!(
            "E2104: 帧缓冲长度 {} 与尺寸 {}x{} RGBA 不一致",
            buffer.len(),
            frame_w,
            frame_h
        ));
    }
    // 裁到客户区，保证坐标系与用户脚本集成的客户区坐标一致
    if let Some((x, y, w, h)) = client_crop(winsitter, session_id, hwnd, frame_w, frame_h)? {
        let cropped = crop_rgba(&buffer, frame_w, x, y, w, h);
        return Ok((cropped, w, h));
    }
    Ok((buffer, frame_w, frame_h))
}
