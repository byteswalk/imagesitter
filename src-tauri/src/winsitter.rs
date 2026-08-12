//! winsitter C ABI 集成层：运行时加载预编译的 winsitter.dll，不编译 SDK 源码。
//!
//! 结构体布局、函数签名与错误码必须与 winsitter 仓库的 ffi 定义保持一致
//! （window_session_api / stream_capture_api / once_capture_api / window_api）。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 成功。
pub const WINSITTER_OK: i32 = 0;
/// 输出缓冲不足；查询尺寸场景属于正常流程。
pub const WINSITTER_ERR_BUFFER_TOO_SMALL: i32 = -1008;
/// 参数非法；旧版 DLL 不识别新增发现标志时会返回此码。
pub const WINSITTER_ERR_INVALID_ARGUMENT: i32 = -1001;
/// 启动捕获时目标窗口已最小化，无法产生可靠画面（与 SDK 同名错误码一致，0.5.5 新增）。
pub const WINSITTER_ERR_CAPTURE_WINDOW_MINIMIZED: i32 = -1109;
/// Windows 拒绝窗口控制请求，通常需要提升调用方权限（与 SDK 同名错误码一致，0.5.5 新增）。
pub const WINSITTER_ERR_WINDOW_ACCESS_DENIED: i32 = -1507;
/// 窗口处于最小化状态（与 SDK `WINDOW_STATE_MINIMIZED` 一致）。
pub const WINDOW_STATE_MINIMIZED: u32 = 2;

/// 窗口被 cloaked（如其它虚拟桌面/隐藏的 UWP 窗口，与 SDK `WINDOW_INFO_CLOAKED` 一致）。
pub const WINDOW_INFO_CLOAKED: u32 = 1 << 5;

/// 标题缓冲容量（与 SDK `WINDOW_FIND_TITLE_CAPACITY` 一致）。
pub const WINDOW_FIND_TITLE_CAPACITY: usize = 512;
/// 类名缓冲容量（与 SDK `WINDOW_FIND_CLASS_NAME_CAPACITY` 一致）。
pub const WINDOW_FIND_CLASS_NAME_CAPACITY: usize = 256;
/// 标题按子串匹配。
pub const WINDOW_FIND_TITLE_CONTAINS: u32 = 1 << 0;
/// 匹配大小写不敏感。
pub const WINDOW_FIND_CASE_INSENSITIVE: u32 = 1 << 2;
/// 仅返回可见窗口。
pub const WINDOW_FIND_VISIBLE_ONLY: u32 = 1 << 3;
/// 发现结果排除 cloaked 窗口（与 SDK 同名标志一致，0.5.5 新增）。
pub const WINDOW_FIND_EXCLUDE_CLOAKED: u32 = 1 << 6;
/// 发现结果排除带 owner 的浮层窗口（与 SDK 同名标志一致，0.5.5 新增）。
pub const WINDOW_FIND_EXCLUDE_OWNED: u32 = 1 << 7;

/// 窗口会话绑定选项（与 SDK `WindowSessionOptions` 布局一致）。
#[repr(C)]
#[derive(Clone)]
pub struct WindowSessionOptions {
    pub struct_size: u32,
    pub flags: u32,
    pub target_response_timeout_ms: u32,
    pub reserved: [u32; 5],
}

impl Default for WindowSessionOptions {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            target_response_timeout_ms: 250,
            reserved: [0; 5],
        }
    }
}

/// 窗口发现入参（与 SDK `WindowFindOptions` 布局一致）。
#[repr(C)]
#[derive(Clone)]
pub struct WindowFindOptions {
    pub struct_size: u32,
    pub flags: u32,
    pub process_id: u32,
    pub reserved0: u32,
    pub title_utf8: [u8; WINDOW_FIND_TITLE_CAPACITY],
    pub class_name_utf8: [u8; WINDOW_FIND_CLASS_NAME_CAPACITY],
    pub reserved: [u32; 4],
}

impl Default for WindowFindOptions {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            process_id: 0,
            reserved0: 0,
            title_utf8: [0; WINDOW_FIND_TITLE_CAPACITY],
            class_name_utf8: [0; WINDOW_FIND_CLASS_NAME_CAPACITY],
            reserved: [0; 4],
        }
    }
}

/// 窗口恢复入参（与 SDK `WindowRestoreOptions` 布局一致）。
#[repr(C)]
#[derive(Clone)]
pub struct WindowRestoreOptions {
    pub struct_size: u32,
    pub flags: u32,
    pub reserved: [u32; 6],
}

impl Default for WindowRestoreOptions {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            reserved: [0; 6],
        }
    }
}

/// 窗口动作结果（与 SDK `WindowActionResult` 布局一致）。
#[repr(C)]
#[derive(Clone, Default)]
pub struct WindowActionResult {
    pub struct_size: u32,
    pub abi_version: u32,
    pub action: u32,
    pub before_state: u32,
    pub after_state: u32,
    pub flags: u32,
    pub window_handle: usize,
    pub os_error: u32,
    pub reserved: [u32; 7],
}

/// 窗口发现结果（与 SDK `WindowFindResult` 布局一致）。
#[repr(C)]
#[derive(Clone)]
pub struct WindowFindResult {
    pub struct_size: u32,
    pub abi_version: u32,
    pub window_handle: usize,
    pub parent_handle: usize,
    pub owner_handle: usize,
    pub process_id: u32,
    pub thread_id: u32,
    pub state: u32,
    pub flags: u32,
    pub title_length: u32,
    pub class_name_length: u32,
    pub dpi: u32,
    pub title_utf8: [u8; WINDOW_FIND_TITLE_CAPACITY],
    pub class_name_utf8: [u8; WINDOW_FIND_CLASS_NAME_CAPACITY],
    pub reserved: [u32; 4],
}

/// 窗口实时状态（与 SDK `WindowInfo` 布局一致）。
#[repr(C)]
#[derive(Clone, Default)]
pub struct WindowInfo {
    pub struct_size: u32,
    pub abi_version: u32,
    pub window_handle: usize,
    pub process_id: u32,
    pub thread_id: u32,
    pub state: u32,
    pub flags: u32,
    /// 扩展窗口帧左边界（屏幕物理像素）。
    pub left: i32,
    /// 扩展窗口帧上边界。
    pub top: i32,
    /// 扩展窗口帧右边界。
    pub right: i32,
    /// 扩展窗口帧下边界。
    pub bottom: i32,
    /// 客户区宽度。
    pub client_width: u32,
    /// 客户区高度。
    pub client_height: u32,
    pub dpi: u32,
    pub monitor_handle: usize,
    pub os_error: u32,
    pub reserved: [u32; 7],
}

type BindWindowFn = unsafe extern "C" fn(usize, *const WindowSessionOptions, *mut u64) -> i32;
type ReleaseWindowFn = unsafe extern "C" fn(u64) -> i32;
type CaptureStartFn = unsafe extern "C" fn(u64) -> i32;
type CaptureRgbaIntoFn =
    unsafe extern "C" fn(u64, *mut u8, i32, *mut i32, *mut i32, *mut i32) -> i32;
type WindowFindFn =
    unsafe extern "C" fn(*const WindowFindOptions, *mut WindowFindResult, u32, *mut u32) -> i32;
type WindowRestoreFn =
    unsafe extern "C" fn(u64, *const WindowRestoreOptions, *mut WindowActionResult) -> i32;
type WindowGetInfoFn = unsafe extern "C" fn(u64, *mut WindowInfo) -> i32;

/// 已解析的 winsitter 导出符号；`_lib` 保证 DLL 在进程生命周期内不被卸载。
pub struct Symbols {
    _lib: libloading::Library,
    bind_window: BindWindowFn,
    release_window: ReleaseWindowFn,
    capture_start: CaptureStartFn,
    capture_rgba_into: CaptureRgbaIntoFn,
    window_find: WindowFindFn,
    window_restore: WindowRestoreFn,
    window_get_info: WindowGetInfoFn,
}

static SDK: OnceLock<Result<Symbols, String>> = OnceLock::new();

/// 获取全局 winsitter 符号表；首次调用时按候选路径加载 winsitter.dll。
pub fn sdk() -> Result<&'static Symbols, String> {
    SDK.get_or_init(|| {
        let mut tried = Vec::new();
        for path in candidate_paths() {
            if !path.is_file() {
                continue;
            }
            tried.push(path.display().to_string());
            match Symbols::load(&path) {
                Ok(symbols) => return Ok(symbols),
                Err(error) => return Err(format!("E0002: 加载 {} 失败：{error}", path.display())),
            }
        }
        Err(format!(
            "E0001: 未找到 winsitter.dll，已尝试：{}",
            if tried.is_empty() {
                "无候选路径存在".to_string()
            } else {
                tried.join("; ")
            }
        ))
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// DLL 候选位置：环境变量 > 可执行文件旁。
/// 构建脚本和 Tauri bundle 会保证开发版与安装版的 DLL 均位于 exe 旁。
fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(custom) = std::env::var_os("WINSITTER_DLL") {
        paths.push(PathBuf::from(custom));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("winsitter.dll"));
        }
    }
    paths
}

impl Symbols {
    fn load(path: &PathBuf) -> Result<Self, String> {
        unsafe {
            let lib = libloading::Library::new(path)
                .map_err(|error| format!("LoadLibrary 失败：{error}"))?;
            let bind_window = *symbol::<BindWindowFn>(&lib, b"winsitter_bind_window\0")?;
            let release_window = *symbol::<ReleaseWindowFn>(&lib, b"winsitter_release_window\0")?;
            let capture_start = *symbol::<CaptureStartFn>(&lib, b"winsitter_capture_start\0")?;
            let capture_rgba_into =
                *symbol::<CaptureRgbaIntoFn>(&lib, b"winsitter_capture_rgba_into\0")?;
            let window_find = *symbol::<WindowFindFn>(&lib, b"winsitter_window_find\0")?;
            let window_restore = *symbol::<WindowRestoreFn>(&lib, b"winsitter_window_restore\0")?;
            let window_get_info = *symbol::<WindowGetInfoFn>(&lib, b"winsitter_window_get_info\0")?;
            Ok(Self {
                _lib: lib,
                bind_window,
                release_window,
                capture_start,
                capture_rgba_into,
                window_find,
                window_restore,
                window_get_info,
            })
        }
    }

    /// 绑定 HWND 得到 session id。
    pub fn bind_window(&self, hwnd: usize, options: &WindowSessionOptions) -> Result<u64, i32> {
        let mut session_id = 0u64;
        let code = unsafe { (self.bind_window)(hwnd, options, &mut session_id) };
        if code == WINSITTER_OK {
            Ok(session_id)
        } else {
            Err(code)
        }
    }

    /// 释放窗口会话。
    pub fn release_window(&self, session_id: u64) -> i32 {
        unsafe { (self.release_window)(session_id) }
    }

    /// 启动持续捕获流并等待首帧。
    pub fn capture_start(&self, session_id: u64) -> i32 {
        unsafe { (self.capture_start)(session_id) }
    }

    /// 恢复窗口 placement 与相对 Z 序快照，不抢占前景。
    pub fn window_restore(
        &self,
        session_id: u64,
        options: &WindowRestoreOptions,
    ) -> Result<WindowActionResult, i32> {
        let mut result = WindowActionResult {
            struct_size: std::mem::size_of::<WindowActionResult>() as u32,
            ..Default::default()
        };
        let code = unsafe { (self.window_restore)(session_id, options, &mut result) };
        if code == WINSITTER_OK {
            Ok(result)
        } else {
            Err(code)
        }
    }

    /// 查询窗口实时状态：帧边界、客户区尺寸、DPI 等。
    pub fn window_get_info(&self, session_id: u64) -> Result<WindowInfo, i32> {
        let mut info = WindowInfo {
            struct_size: std::mem::size_of::<WindowInfo>() as u32,
            ..Default::default()
        };
        let code = unsafe { (self.window_get_info)(session_id, &mut info) };
        if code == WINSITTER_OK {
            Ok(info)
        } else {
            Err(code)
        }
    }

    /// 读取最新 RGBA8 帧；缓冲不足时返回 `Err(BUFFER_TOO_SMALL)` 与所需尺寸。
    pub fn capture_rgba_into(
        &self,
        session_id: u64,
        buffer: &mut [u8],
        width: &mut i32,
        height: &mut i32,
        size: &mut i32,
    ) -> i32 {
        unsafe {
            (self.capture_rgba_into)(
                session_id,
                buffer.as_mut_ptr(),
                buffer.len() as i32,
                width,
                height,
                size,
            )
        }
    }

    /// 枚举窗口；自动处理输出缓冲不足的重试。
    pub fn find_windows(&self, options: &WindowFindOptions) -> Result<Vec<WindowFindResult>, i32> {
        let mut capacity = 256u32;
        loop {
            let mut results: Vec<WindowFindResult> = Vec::with_capacity(capacity as usize);
            let mut count = 0u32;
            let code =
                unsafe { (self.window_find)(options, results.as_mut_ptr(), capacity, &mut count) };
            if code == WINSITTER_OK {
                unsafe { results.set_len(count as usize) };
                return Ok(results);
            }
            if code == WINSITTER_ERR_BUFFER_TOO_SMALL {
                // count 已写回总数；不足时按其扩容重试一次
                capacity = count.max(capacity.saturating_mul(2));
                continue;
            }
            return Err(code);
        }
    }
}

unsafe fn symbol<'a, T>(
    lib: &'a libloading::Library,
    name: &[u8],
) -> Result<libloading::Symbol<'a, T>, String> {
    lib.get(name)
        .map_err(|error| format!("缺少导出符号：{error}"))
}
