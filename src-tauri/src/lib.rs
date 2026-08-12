//! ImageSitter 后端：窗口绑定、后台取帧、特征匹配与项目持久化的 Tauri 命令层。

mod commands;
pub mod domain;
mod state;
mod winsitter;

use state::AppState;

/// 组装 Tauri 应用并进入事件循环。
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::windows::list_windows,
            commands::capture::bind_target,
            commands::capture::unbind_target,
            commands::capture::restore_window,
            commands::capture::relaunch_elevated,
            commands::capture::capture_frame_png,
            commands::capture::target_screen_origin,
            commands::capture::sample_points,
            commands::matching::run_match_advanced,
            commands::matching::run_match_png_advanced,
            commands::calibrate::suggest_tolerances_command,
            commands::calibrate::suggest_feature_points_command,
            commands::project::save_project_file,
            commands::project::open_project_file,
            commands::project::list_project_history,
            commands::project::read_project_history,
            commands::project::save_image_png,
            commands::project::import_sample_png,
            commands::project::store_sample_png_data,
            commands::project::load_sample_png,
            commands::project::list_png_files,
            commands::project::audit_project_samples,
            commands::project::cleanup_orphan_samples,
            commands::project::save_text_file,
            commands::project::runtime_diagnostics,
            commands::project::export_diagnostics,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run imagesitter");
}
