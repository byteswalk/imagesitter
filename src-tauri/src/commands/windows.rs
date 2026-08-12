//! 窗口发现命令：列出可绑定的可见顶层窗口。

use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use crate::winsitter::{
    sdk, WindowFindOptions, WINDOW_FIND_CASE_INSENSITIVE, WINDOW_FIND_EXCLUDE_CLOAKED,
    WINDOW_FIND_EXCLUDE_OWNED, WINDOW_FIND_TITLE_CONTAINS, WINDOW_FIND_VISIBLE_ONLY,
    WINDOW_INFO_CLOAKED, WINSITTER_ERR_INVALID_ARGUMENT,
};

/// 前端窗口列表项。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowItem {
    /// 窗口 HWND。
    pub hwnd: usize,
    /// 窗口标题。
    pub title: String,
    /// 窗口类名。
    pub class_name: String,
    /// 进程 ID。
    pub process_id: u32,
    /// 窗口显示状态（winsitter WINDOW_STATE_*）。
    pub state: u32,
    /// 窗口 DPI。
    pub dpi: u32,
}

/// 常见系统/外壳噪音窗口的类名；这些窗口不适合作为捕获目标。
const NOISE_CLASSES: &[&str] = &[
    "Shell_TrayWnd",                // 任务栏
    "Shell_SecondaryTrayWnd",       // 副屏任务栏
    "Shell_CharmWindow",            // 系统侧边弹窗
    "Progman",                      // 桌面
    "WorkerW",                      // 桌面壁纸层
    "tooltips_class32",             // 系统提示气泡
    "DV2ControlHost",               // 桌面预览
    "MultitaskingViewFrame",        // 任务视图
    "TaskListThumbnailWnd",         // 任务栏缩略图
    "ForegroundStaging",            // 系统暂存层
    "XamlExplorerHostIslandWindow", // 资源管理器浮层
    "NativeHWNDHost",               // 系统原生宿主
    "Windows.UI.Core.CoreWindow",   // UWP 后台辅助窗口
    "ImageSitter",                  // 本应用自身
];

/// 枚举当前可见的顶层窗口；可按标题子串过滤。
#[tauri::command]
pub fn list_windows(
    _state: State<AppState>,
    title_filter: Option<String>,
) -> Result<Vec<WindowItem>, String> {
    let mut options = WindowFindOptions {
        flags: WINDOW_FIND_VISIBLE_ONLY
            | WINDOW_FIND_TITLE_CONTAINS
            | WINDOW_FIND_CASE_INSENSITIVE
            | WINDOW_FIND_EXCLUDE_CLOAKED
            | WINDOW_FIND_EXCLUDE_OWNED,
        ..Default::default()
    };
    if let Some(filter) = title_filter {
        let bytes = filter.as_bytes();
        let capacity = options.title_utf8.len().saturating_sub(1);
        let mut length = bytes.len().min(capacity);
        while length > 0 && !filter.is_char_boundary(length) {
            length -= 1;
        }
        options.title_utf8[..length].copy_from_slice(&bytes[..length]);
        options.title_utf8[length] = 0;
    }

    let results = match sdk()?.find_windows(&options) {
        Ok(results) => results,
        // 旧版 DLL 不识别 EXCLUDE_CLOAKED/EXCLUDE_OWNED：回退旧标志，由下方本地过滤兜底
        Err(WINSITTER_ERR_INVALID_ARGUMENT) => {
            options.flags &= !(WINDOW_FIND_EXCLUDE_CLOAKED | WINDOW_FIND_EXCLUDE_OWNED);
            sdk()?
                .find_windows(&options)
                .map_err(|code| format!("E1001: 窗口枚举失败（winsitter 错误码 {code}）"))?
        }
        Err(code) => return Err(format!("E1001: 窗口枚举失败（winsitter 错误码 {code}）")),
    };

    let mut items: Vec<WindowItem> = results
        .into_iter()
        .filter_map(|item| {
            let title_end = (item.title_length as usize).min(item.title_utf8.len());
            let title = String::from_utf8_lossy(&item.title_utf8[..title_end])
                .trim()
                .to_string();
            let class_end = (item.class_name_length as usize).min(item.class_name_utf8.len());
            let class_name =
                String::from_utf8_lossy(&item.class_name_utf8[..class_end]).into_owned();
            // 排除无标题窗口、系统外壳噪音窗口与本应用自身
            if title.is_empty() || NOISE_CLASSES.iter().any(|noise| *noise == class_name) {
                return None;
            }
            // 双重保险：SDK 已在 EXCLUDE_CLOAKED/EXCLUDE_OWNED 下过滤，这里兼容旧版 DLL
            if item.flags & WINDOW_INFO_CLOAKED != 0 {
                return None;
            }
            if item.owner_handle != 0 {
                return None;
            }
            Some(WindowItem {
                hwnd: item.window_handle,
                title,
                class_name,
                process_id: item.process_id,
                state: item.state,
                dpi: item.dpi,
            })
        })
        .collect();
    // 按标题排序，便于在长列表中快速定位
    items.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(items)
}
