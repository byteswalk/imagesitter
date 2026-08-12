//! 项目文件命令：读写 ImageSitter 项目 JSON 与导出图像。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 校验并保存项目 JSON 到用户选择的路径。
#[tauri::command]
pub fn save_project_file(path: String, content: String) -> Result<(), String> {
    let path = sanitize_path(&path)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("E4001: 项目内容不是合法 JSON：{error}"))?;
    let version = value.get("version").and_then(|v| v.as_u64());
    if !matches!(version, Some(1..=4)) {
        return Err("E4002: 项目 version 必须是受支持的 1～4".to_string());
    }
    if value
        .get("objects")
        .is_none_or(|objects| !objects.is_array())
    {
        return Err("E4002: 项目 objects 必须是数组".to_string());
    }
    let pretty = serde_json::to_string_pretty(&value).map_err(|error| format!("E4003: {error}"))?;
    if path.is_file() {
        let current =
            fs::read(&path).map_err(|error| format!("E4004: 读取旧版项目失败：{error}"))?;
        if current != pretty.as_bytes() {
            create_history_snapshot(&path, &current)
                .map_err(|error| format!("E4005: 创建历史快照失败：{error}"))?;
        }
    }
    atomic_write(&path, pretty.as_bytes()).map_err(|error| format!("E4004: 写入失败：{error}"))
}

/// 读取项目 JSON 并做基础结构校验。
#[tauri::command]
pub fn open_project_file(path: String) -> Result<String, String> {
    let path = sanitize_path(&path)?;
    let content = fs::read_to_string(&path).map_err(|error| format!("E4101: 读取失败：{error}"))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("E4102: 文件不是合法 JSON：{error}"))?;
    if !matches!(value.get("version").and_then(|v| v.as_u64()), Some(1..=4))
        || value.get("objects").map(|o| o.is_array()) != Some(true)
    {
        return Err("E4103: 不是有效的 ImageSitter 项目文件".to_string());
    }
    Ok(content)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHistoryEntry {
    pub file_name: String,
    pub saved_at: u64,
    pub size: u64,
}

/// 列出项目最近的自动历史快照（新到旧）。
#[tauri::command]
pub fn list_project_history(project_path: String) -> Result<Vec<ProjectHistoryEntry>, String> {
    let project = PathBuf::from(project_path.trim());
    if project.as_os_str().is_empty() {
        return Err("E4450: 项目路径为空".to_string());
    }
    let directory = history_directory(&project)?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("E4451: 读取历史目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("E4451: 读取历史项失败：{error}"))?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        let saved_at = file_name
            .strip_suffix(".json")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let size = entry.metadata().map(|value| value.len()).unwrap_or(0);
        entries.push(ProjectHistoryEntry {
            file_name,
            saved_at,
            size,
        });
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.saved_at));
    entries.truncate(30);
    Ok(entries)
}

/// 读取某个历史快照；恢复前由前端再走完整格式校验。
#[tauri::command]
pub fn read_project_history(project_path: String, file_name: String) -> Result<String, String> {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || !file_name.ends_with(".json")
    {
        return Err("E4452: 历史快照名无效".to_string());
    }
    let directory = history_directory(Path::new(project_path.trim()))?;
    let content = fs::read_to_string(directory.join(file_name))
        .map_err(|error| format!("E4453: 读取历史快照失败：{error}"))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("E4454: 历史快照已损坏：{error}"))?;
    if !matches!(
        value.get("version").and_then(|item| item.as_u64()),
        Some(1..=4)
    ) || value.get("objects").map(|item| item.is_array()) != Some(true)
    {
        return Err("E4454: 历史快照不是有效项目".to_string());
    }
    Ok(content)
}

fn history_directory(project: &Path) -> Result<PathBuf, String> {
    let parent = project
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or_else(|| "E4450: 项目路径缺少父目录".to_string())?;
    let stem = project
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("imagesitter-project");
    Ok(parent.join(format!("{stem}.history")))
}

fn create_history_snapshot(project: &Path, content: &[u8]) -> std::io::Result<()> {
    // 不把已损坏的主文件带入历史链。
    let valid = serde_json::from_slice::<serde_json::Value>(content)
        .ok()
        .is_some_and(|value| {
            value
                .get("objects")
                .is_some_and(|objects| objects.is_array())
        });
    if !valid {
        return Ok(());
    }
    let directory = history_directory(project)
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
    fs::create_dir_all(&directory)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    atomic_write(&directory.join(format!("{timestamp}.json")), content)?;

    let mut snapshots = fs::read_dir(&directory)?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("json"))
                .then_some(path)
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|first, second| second.file_name().cmp(&first.file_name()));
    for old in snapshots.into_iter().skip(30) {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// 把 Base64 编码的 PNG 写入用户选择的路径（右键保存选区图像用）。
#[tauri::command]
pub fn save_image_png(path: String, png_base64: String) -> Result<(), String> {
    let path = sanitize_path(&path)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|error| format!("E4301: 图像数据不是合法 Base64：{error}"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("E4302: 图像数据不是 PNG 格式".to_string());
    }
    atomic_write(&path, &bytes).map_err(|error| format!("E4303: 写入失败：{error}"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSample {
    pub png_data_url: String,
    pub relative_path: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

fn inspect_png(bytes: &[u8]) -> Result<(u32, u32, String), String> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("E4402: 样本不是 PNG 格式".to_string());
    }
    if bytes.len() > 24 * 1024 * 1024 {
        return Err("E4403: 单张 PNG 超过 24 MiB 上限".to_string());
    }
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|error| format!("E4404: PNG 解码失败：{error}"))?;
    if u64::from(image.width()) * u64::from(image.height()) > 100_000_000 {
        return Err("E4405: PNG 像素数量超过安全上限".to_string());
    }
    let hash = format!("{:x}", Sha256::digest(bytes));
    Ok((image.width(), image.height(), hash))
}

/// 导入 PNG；external 模式会复制到项目旁的受管 `<项目名>.samples` 目录。
#[tauri::command]
pub fn import_sample_png(
    project_path: Option<String>,
    source_path: String,
    storage: String,
) -> Result<ImportedSample, String> {
    let source = PathBuf::from(source_path.trim());
    if !source.is_file() {
        return Err("E4401: 样本文件不存在".to_string());
    }
    let bytes = fs::read(&source).map_err(|error| format!("E4401: 读取样本失败：{error}"))?;
    let (width, height, sha256) = inspect_png(&bytes)?;
    if storage == "embedded" {
        return Ok(ImportedSample {
            png_data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ),
            relative_path: String::new(),
            width,
            height,
            sha256,
        });
    }
    if storage != "external" {
        return Err("E4406: storage 必须为 embedded 或 external".to_string());
    }
    let project = project_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "E4407: 外部样本要求先保存项目".to_string())?;
    let parent = project
        .parent()
        .filter(|path| path.is_dir())
        .ok_or_else(|| "E4407: 项目父目录不存在".to_string())?;
    let stem = project
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("imagesitter-project");
    let folder_name = format!("{stem}.samples");
    let samples_dir = parent.join(&folder_name);
    fs::create_dir_all(&samples_dir)
        .map_err(|error| format!("E4408: 创建样本目录失败：{error}"))?;
    let original = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sample.png");
    let safe_name: String = original
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || ".-_".contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect();
    let target_name = format!("{}-{}", &sha256[..12], safe_name);
    let target = samples_dir.join(&target_name);
    if !target.exists() {
        atomic_write(&target, &bytes).map_err(|error| format!("E4409: 写入样本失败：{error}"))?;
    }
    Ok(ImportedSample {
        png_data_url: String::new(),
        relative_path: format!("{folder_name}/{target_name}"),
        width,
        height,
        sha256,
    })
}

/// 把实时捕获得到的 Data URL 规范化为内嵌样本或写入受管外部样本目录。
#[tauri::command]
pub fn store_sample_png_data(
    project_path: Option<String>,
    png_data_url: String,
    storage: String,
) -> Result<ImportedSample, String> {
    let encoded = png_data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "E4430: 捕获样本不是 PNG Data URL".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| format!("E4430: 捕获样本 Base64 无效：{error}"))?;
    let (width, height, sha256) = inspect_png(&bytes)?;
    if storage == "embedded" {
        return Ok(ImportedSample {
            png_data_url,
            relative_path: String::new(),
            width,
            height,
            sha256,
        });
    }
    if storage != "external" {
        return Err("E4406: storage 必须为 embedded 或 external".to_string());
    }
    let project = project_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "E4407: 外部样本要求先保存项目".to_string())?;
    let parent = project
        .parent()
        .filter(|path| path.is_dir())
        .ok_or_else(|| "E4407: 项目父目录不存在".to_string())?;
    let stem = project
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("imagesitter-project");
    let folder_name = format!("{stem}.samples");
    let samples_dir = parent.join(&folder_name);
    fs::create_dir_all(&samples_dir)
        .map_err(|error| format!("E4408: 创建样本目录失败：{error}"))?;
    let target_name = format!("{}-capture.png", &sha256[..12]);
    let target = samples_dir.join(&target_name);
    if !target.exists() {
        atomic_write(&target, &bytes).map_err(|error| format!("E4409: 写入样本失败：{error}"))?;
    }
    Ok(ImportedSample {
        png_data_url: String::new(),
        relative_path: format!("{folder_name}/{target_name}"),
        width,
        height,
        sha256,
    })
}

/// 读取受管外部样本并校验其哈希。
#[tauri::command]
pub fn load_sample_png(
    project_path: String,
    relative_path: String,
    expected_sha256: String,
) -> Result<ImportedSample, String> {
    let relative = Path::new(&relative_path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("E4410: 外部样本路径必须是安全的相对路径".to_string());
    }
    let project = PathBuf::from(project_path);
    let parent = project
        .parent()
        .ok_or_else(|| "E4411: 项目路径缺少父目录".to_string())?;
    let target = parent.join(relative);
    let bytes = fs::read(&target).map_err(|error| format!("E4412: 外部样本缺失：{error}"))?;
    let (width, height, sha256) = inspect_png(&bytes)?;
    if !expected_sha256.is_empty() && !sha256.eq_ignore_ascii_case(&expected_sha256) {
        return Err("E4413: 外部样本内容已变化（SHA-256 不一致）".to_string());
    }
    Ok(ImportedSample {
        png_data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
        relative_path,
        width,
        height,
        sha256,
    })
}

/// 枚举目录中的 PNG，供批量导入；最多返回 2000 个文件。
#[tauri::command]
pub fn list_png_files(directory: String) -> Result<Vec<String>, String> {
    let directory = PathBuf::from(directory);
    if !directory.is_dir() {
        return Err("E4414: 导入路径不是目录".to_string());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| format!("E4415: 无法读取目录：{error}"))?
    {
        let entry = entry.map_err(|error| format!("E4415: 无法读取目录项：{error}"))?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        {
            files.push(path.display().to_string());
            if files.len() >= 2000 {
                break;
            }
        }
    }
    files.sort();
    Ok(files)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleReference {
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleAudit {
    pub missing: Vec<String>,
    pub modified: Vec<String>,
    pub orphaned: Vec<String>,
}

fn managed_sample_target(project: &Path, relative: &str) -> Result<PathBuf, String> {
    let parent = project
        .parent()
        .filter(|value| value.is_dir())
        .ok_or_else(|| "E4470: 项目父目录不存在".to_string())?;
    let stem = project
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("imagesitter-project");
    let folder = format!("{stem}.samples");
    let relative_path = Path::new(relative);
    let components = relative_path.components().collect::<Vec<_>>();
    if components.len() != 2
        || components[0].as_os_str() != std::ffi::OsStr::new(&folder)
        || !matches!(components[1], std::path::Component::Normal(_))
    {
        return Err("E4471: 样本不在当前项目的受管目录中".to_string());
    }
    Ok(parent.join(relative_path))
}

/// 检查外置样本的缺失、内容变更和无引用文件。
#[tauri::command]
pub fn audit_project_samples(
    project_path: String,
    references: Vec<SampleReference>,
) -> Result<SampleAudit, String> {
    let project = PathBuf::from(project_path.trim());
    let mut missing = Vec::new();
    let mut modified = Vec::new();
    let mut referenced = std::collections::HashSet::new();
    for reference in references {
        let target = managed_sample_target(&project, &reference.relative_path)?;
        referenced.insert(reference.relative_path.replace('\\', "/"));
        let Ok(bytes) = fs::read(&target) else {
            missing.push(reference.relative_path);
            continue;
        };
        if !reference.sha256.is_empty() {
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if !actual.eq_ignore_ascii_case(&reference.sha256) {
                modified.push(reference.relative_path);
            }
        }
    }

    let parent = project
        .parent()
        .ok_or_else(|| "E4470: 项目路径缺少父目录".to_string())?;
    let stem = project
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("imagesitter-project");
    let folder = format!("{stem}.samples");
    let samples_dir = parent.join(&folder);
    let mut orphaned = Vec::new();
    if samples_dir.is_dir() {
        for entry in fs::read_dir(&samples_dir)
            .map_err(|error| format!("E4472: 读取样本目录失败：{error}"))?
        {
            let entry = entry.map_err(|error| format!("E4472: 读取样本项失败：{error}"))?;
            if !entry.path().is_file() {
                continue;
            }
            let relative = format!("{folder}/{}", entry.file_name().to_string_lossy());
            if !referenced.contains(&relative) {
                orphaned.push(relative);
            }
        }
    }
    missing.sort();
    modified.sort();
    orphaned.sort();
    Ok(SampleAudit {
        missing,
        modified,
        orphaned,
    })
}

/// 仅删除用户确认过的、当前项目受管样本目录中的普通文件。
#[tauri::command]
pub fn cleanup_orphan_samples(
    project_path: String,
    relative_paths: Vec<String>,
) -> Result<usize, String> {
    let project = PathBuf::from(project_path.trim());
    let mut removed = 0usize;
    for relative in relative_paths {
        let target = managed_sample_target(&project, &relative)?;
        if target.is_file() {
            fs::remove_file(&target)
                .map_err(|error| format!("E4473: 删除无引用样本失败：{error}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// 保存 JSON/CSV/HTML 等文本报告。
#[tauri::command]
pub fn save_text_file(path: String, content: String) -> Result<(), String> {
    if content.len() > 64 * 1024 * 1024 {
        return Err("E4420: 报告超过 64 MiB 上限".to_string());
    }
    let path = sanitize_path(&path)?;
    atomic_write(&path, content.as_bytes()).map_err(|error| format!("E4421: 写入报告失败：{error}"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    pub app_version: &'static str,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub winsitter_dll_present: bool,
    pub generated_at: u64,
}

/// 返回不含截图、项目内容或凭据的运行环境摘要。
#[tauri::command]
pub fn runtime_diagnostics() -> RuntimeDiagnostics {
    let dll_present = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("winsitter.dll")))
        .is_some_and(|path| path.is_file());
    RuntimeDiagnostics {
        app_version: env!("CARGO_PKG_VERSION"),
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        winsitter_dll_present: dll_present,
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

/// 导出隐私最小化的诊断 JSON；project_summary 只由前端传入计数摘要。
#[tauri::command]
pub fn export_diagnostics(path: String, project_summary: serde_json::Value) -> Result<(), String> {
    let path = sanitize_path(&path)?;
    let payload = serde_json::json!({
        "diagnostics": runtime_diagnostics(),
        "projectSummary": project_summary,
        "privacy": "No screenshots, project paths, window titles, or credentials are included."
    });
    let content = serde_json::to_vec_pretty(&payload).map_err(|error| format!("E4460: {error}"))?;
    atomic_write(&path, &content).map_err(|error| format!("E4461: 导出诊断信息失败：{error}"))
}

/// 规范化路径并阻止写入目录或空路径。
fn sanitize_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("E4201: 路径为空".to_string());
    }
    let path = PathBuf::from(trimmed);
    let canonical_parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "E4202: 路径缺少父目录".to_string())?;
    if !canonical_parent.exists() {
        return Err(format!("E4203: 目录不存在：{}", canonical_parent.display()));
    }
    if !canonical_parent.is_dir() {
        return Err(format!(
            "E4203: 父路径不是目录：{}",
            canonical_parent.display()
        ));
    }
    if path.is_dir() {
        return Err("E4205: 目标路径是目录，不能作为文件写入".to_string());
    }
    if path.extension().is_none() {
        return Err("E4204: 请为文件指定扩展名（建议 .json）".to_string());
    }
    Ok(path)
}

/// 同目录临时文件落盘后原子替换目标，避免崩溃或断电留下半截项目文件。
fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "目标路径缺少父目录")
    })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("imagesitter-project");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), nonce));

    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_json(name: &str) -> String {
        serde_json::json!({
            "version": 4,
            "target": {
                "windowTitle": "",
                "className": "",
                "processId": 0,
                "frameWidth": 0,
                "frameHeight": 0,
                "baselineDpi": 0
            },
            "objects": [],
            "replayCases": [],
            "testName": name
        })
        .to_string()
    }

    #[test]
    fn v4_save_creates_history_before_overwrite() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("imagesitter-history-test-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("project.json");
        save_project_file(path.display().to_string(), project_json("first")).unwrap();
        save_project_file(path.display().to_string(), project_json("second")).unwrap();
        let history = list_project_history(path.display().to_string()).unwrap();
        assert_eq!(history.len(), 1);
        let content =
            read_project_history(path.display().to_string(), history[0].file_name.clone()).unwrap();
        assert!(content.contains("first"));
        assert!(open_project_file(path.display().to_string())
            .unwrap()
            .contains("second"));
        fs::remove_dir_all(directory).unwrap();
    }
}
