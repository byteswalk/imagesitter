//! 应用级共享状态：已绑定的目标窗口会话注册表。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 一个已绑定的目标窗口会话；字段保留完整会话档案供诊断与后续能力使用。
#[allow(dead_code)]
pub struct TargetSession {
    /// ImageSitter 内部分配的目标 ID。
    pub target_id: u64,
    /// winsitter 窗口 session ID。
    pub session_id: u64,
    /// 目标窗口 HWND。
    pub hwnd: usize,
    /// 绑定时记录的窗口标题，用于界面展示。
    pub title: String,
}

/// 全局应用状态；由 Tauri `manage` 注入。
#[derive(Default)]
pub struct AppState {
    /// target_id -> 目标会话。
    pub targets: Mutex<HashMap<u64, Arc<Mutex<TargetSession>>>>,
    /// 下一个可用的 target_id。
    pub next_target_id: Mutex<u64>,
}
