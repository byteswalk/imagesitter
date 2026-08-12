# ImageSitter

ImageSitter 是面向游戏辅助和 Windows 桌面自动化开发者的图像特征标记、校准与离线回归工具。它使用稀疏像素特征而非整图卷积，适合需要低延迟判断、对象存在多种视觉状态、画面带蒙版或透明度变化的场景。

## 核心能力

- 后台绑定和捕获目标窗口，不抢前台；记录标注基准尺寸与 DPI。
- 一个对象可定义多个可启停的视觉状态；支持 RGB 容差、alpha 策略、`minMatch` 和排除点。
- 固定坐标、按客户区比例缩放、锚点定位三种适配方式，并可在邻域和小幅缩放范围内搜索最佳命中。
- 从多张正样本和负样本推导参考色、容差、质量分和新的高区分度候选点。
- 单张或定时连续录制，目录批量导入，多对象联合期望，三路并行回放，可取消并导出 JSON/CSV/HTML 报告。
- 回放 PNG 可内嵌或存放在受管 `.samples` 目录；SHA-256 校验可发现缺失或被修改的样本。
- 自动恢复、100 步受限撤销/重做、保存前历史快照、对象模板、项目规则对比/合并和便携单文件导出。
- 提供无界面 CLI，便于 CI 校验、单帧匹配和整项目回归。

## 开发与验证

要求 Node.js 22、pnpm 10、稳定版 Rust，以及 Windows SDK。

```powershell
pnpm install
pnpm check
pnpm build
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

桌面开发需要获得授权的 x64 `winsitter.dll`：

```powershell
$env:WINSITTER_DLL = "D:\path\to\winsitter.dll"
pnpm tauri dev
```

公开源码仓库不包含该专有 DLL。只运行格式测试、领域测试或 CLI 构建时，构建脚本会生成空资源占位；它不能用于窗口捕获。正式打包会强制检查 DLL：

```powershell
$env:WINSITTER_DLL = "D:\path\to\winsitter.dll"
pnpm package:windows
```

打包脚本会在 `release/` 生成 NSIS 安装程序、便携目录、便携 ZIP 和 `SHA256SUMS.txt`；该目录只保留当前交付版本。对外分发前必须确认拥有 `winsitter.dll` 的分发许可；Windows 代码签名还需要发布方证书。

## CLI

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --features cli --bin imagesitter-cli -- validate project.json
cargo run --manifest-path src-tauri/Cargo.toml --features cli --bin imagesitter-cli -- match project.json object-id frame.png
cargo run --manifest-path src-tauri/Cargo.toml --features cli --bin imagesitter-cli -- test project.json
```

`test` 在断言失败时返回退出码 2，输入或执行错误返回 1，全部通过返回 0；输出为机器可读 JSON。

## 格式与安全边界

- 当前项目格式为 v4；应用可读 v1～v4，保存时迁移到 v4。
- `region` 使用 LTRB 左闭右开；点坐标相对区域左上角。
- 单 PNG 上限 24 MiB / 1 亿像素，内嵌样本总编码上限 64 MiB。
- 外置样本只允许项目目录下的安全相对路径，并在加载时校验 SHA-256。
- 诊断导出不包含截图、项目路径、窗口标题或账号凭据。

详细约定见 [项目格式规范](docs/spec-format.md)、[v4 JSON Schema](docs/project-v4.schema.json)、[设计决策](docs/decisions.md) 和 [1.0.0 发布说明](docs/release-1.0.0.md)。

## 发布说明

自动更新需要稳定的 HTTPS 更新端点和 Tauri 更新签名密钥；代码签名需要 Windows 代码签名证书。这些安全材料不会放入源码仓库，也不会由构建过程伪造。
