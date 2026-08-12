//! 应用级共享状态：已绑定的目标窗口会话注册表。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 一个已绑定的目标窗口会话。
pub struct TargetSession {
    /// winsitter 窗口 session ID。
    pub session_id: u64,
    /// 目标窗口 HWND。
    pub hwnd: usize,
}

/// 全局应用状态；由 Tauri `manage` 注入。
#[derive(Default)]
pub struct AppState {
    /// target_id -> 目标会话。
    pub targets: Mutex<HashMap<u64, Arc<Mutex<TargetSession>>>>,
    /// 下一个可用的 target_id。
    pub next_target_id: Mutex<u64>,
}
