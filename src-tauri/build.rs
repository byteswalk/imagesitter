fn main() {
    // 部分构建环境未向 tauri-winres 传递 RC.EXE 位置，这里在 SDK 安装目录中兜底查找
    if std::env::var_os("RC").is_none() {
        if let Some(rc) = find_rc_exe() {
            std::env::set_var("RC", &rc);
        }
    }
    prepare_winsitter_dll();
    tauri_build::build()
}

/// 查找可用的 x64 rc.exe：优先工作区内副本（部分安全策略拦截 Program Files 路径），
/// 其次在 Windows SDK 常见安装路径下取最新版本
fn find_rc_exe() -> Option<String> {
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let local = format!("{manifest}\\tools\\rc\\rc.exe");
        if std::path::Path::new(&local).exists() {
            return Some(local);
        }
    }
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files (x86)\Windows Kits\11\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    let mut best: Option<String> = None;
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("10.") {
                continue;
            }
            let candidate = format!("{}\\{}\\x64\\rc.exe", root, name);
            if !std::path::Path::new(&candidate).exists() {
                continue;
            }
            // 字典序即版本号序，取最新
            match &best {
                Some(prev) if prev >= &candidate => {}
                _ => best = Some(candidate),
            }
        }
    }
    best
}

/// 准备 Tauri 资源。`tauri.conf.json` 会把该文件复制到开发版 exe 旁，
/// 并写入安装包；不再自行猜测 Cargo target 目录。
fn prepare_winsitter_dll() {
    use std::path::{Path, PathBuf};

    println!("cargo:rerun-if-env-changed=WINSITTER_DLL");
    println!("cargo:rerun-if-env-changed=IMAGESITTER_REQUIRE_WINSITTER");
    let manifest =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 未设置"));
    let staged = manifest.join("resources").join("winsitter.dll");
    println!("cargo:rerun-if-changed={}", staged.display());
    let mut candidates = Vec::new();
    if let Some(custom) = std::env::var_os("WINSITTER_DLL") {
        candidates.push(PathBuf::from(custom));
    }
    // 已准备过的副本可用于离线重复构建。
    candidates.push(staged.clone());

    let source = candidates.iter().find(|candidate| {
        candidate
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 0)
    });
    let Some(source) = source else {
        if std::env::var("IMAGESITTER_REQUIRE_WINSITTER").as_deref() == Ok("1") {
            panic!("发布构建找不到 winsitter.dll；请设置 WINSITTER_DLL");
        }
        // 公开源码不分发专有 DLL。测试/CLI 构建用空占位满足 Tauri 资源解析；
        // 真正的开发或打包通过 WINSITTER_DLL 显式提供二进制。
        if !staged.exists() {
            std::fs::create_dir_all(staged.parent().unwrap())
                .expect("无法创建 src-tauri/resources");
            std::fs::write(&staged, []).expect("无法创建 winsitter.dll 构建占位");
        }
        println!("cargo:warning=[build.rs] 未提供 winsitter.dll；仅可用于测试和 CLI 构建");
        return;
    };
    if source != &staged {
        println!("cargo:rerun-if-changed={}", source.display());
    }

    if source != &staged {
        std::fs::create_dir_all(staged.parent().unwrap()).expect("无法创建 src-tauri/resources");
        copy_if_changed(source, &staged).unwrap_or_else(|error| {
            panic!(
                "准备 winsitter.dll 失败（{} -> {}）：{error}",
                source.display(),
                staged.display()
            )
        });
    }
    println!(
        "cargo:warning=[build.rs] winsitter.dll 资源已准备：{}",
        staged.display()
    );

    fn copy_if_changed(source: &Path, target: &Path) -> std::io::Result<()> {
        let unchanged = target.is_file()
            && std::fs::metadata(source)?.len() == std::fs::metadata(target)?.len()
            && std::fs::read(source)? == std::fs::read(target)?;
        if !unchanged {
            std::fs::copy(source, target)?;
        }
        Ok(())
    }
}
