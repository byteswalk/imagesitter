/**
 * 后端 IPC 封装：所有 Tauri 命令的类型安全入口。
 * 命令契约版本 v1；后端错误统一为 "E####: 说明" 字符串。
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  BoundTarget,
  FramePayload,
  MatchReport,
  FeatureGroup,
  PointSuggestion,
  FeatureCandidate,
  Region,
  ScreenOrigin,
  WindowItem,
} from "./types";

export function listWindows(titleFilter?: string): Promise<WindowItem[]> {
  return invoke("list_windows", { titleFilter: titleFilter ?? null });
}

export function bindTarget(
  hwnd: number,
  title: string,
  windowState: number,
): Promise<BoundTarget> {
  return invoke("bind_target", { hwnd, title, windowState });
}

export function unbindTarget(targetId: number): Promise<void> {
  return invoke("unbind_target", { targetId });
}

/** 点击窗口列表项时立即恢复最小化窗口（winsitter 临时会话，不抢前景） */
export function restoreWindow(hwnd: number): Promise<void> {
  return invoke("restore_window", { hwnd });
}

/** 动态弹 UAC 提权重启本程序 */
export function relaunchElevated(): Promise<void> {
  return invoke("relaunch_elevated");
}

export function captureFramePng(targetId: number): Promise<FramePayload> {
  return invoke("capture_frame_png", { targetId });
}

export function targetScreenOrigin(targetId: number): Promise<ScreenOrigin> {
  return invoke("target_screen_origin", { targetId });
}

export function samplePoints(
  targetId: number,
  points: { x: number; y: number }[],
): Promise<([number, number, number, number] | null)[]> {
  return invoke("sample_points", { targetId, points });
}

export function suggestTolerances(
  samples: [number, number, number, number][][],
  negativeSamples: [number, number, number, number][][],
  pointsPerSample: number,
  margin: number,
): Promise<PointSuggestion[]> {
  return invoke("suggest_tolerances_command", {
    samples,
    negativeSamples,
    pointsPerSample,
    margin,
  });
}

export function suggestFeaturePoints(
  positivePngs: string[],
  negativePngs: string[],
  region: Region,
  limit: number,
  minimumDistance: number,
): Promise<FeatureCandidate[]> {
  return invoke("suggest_feature_points_command", {
    positivePngs,
    negativePngs,
    region,
    limit,
    minimumDistance,
  });
}

export function runMatchAdvanced(
  targetId: number,
  region: Region,
  groups: FeatureGroup[],
  searchRadius: number,
  scaleSearchPercent: number,
): Promise<MatchReport> {
  return invoke("run_match_advanced", {
    targetId,
    region,
    groups,
    searchRadius,
    scaleSearchPercent,
  });
}

export function runMatchPngAdvanced(
  pngDataUrl: string,
  region: Region,
  groups: FeatureGroup[],
  searchRadius: number,
  scaleSearchPercent: number,
): Promise<MatchReport> {
  return invoke("run_match_png_advanced", {
    pngDataUrl,
    region,
    groups,
    searchRadius,
    scaleSearchPercent,
  });
}

export function saveProjectFile(path: string, content: string): Promise<void> {
  return invoke("save_project_file", { path, content });
}

export interface ProjectHistoryEntry {
  fileName: string;
  savedAt: number;
  size: number;
}

export function listProjectHistory(projectPath: string): Promise<ProjectHistoryEntry[]> {
  return invoke("list_project_history", { projectPath });
}

export async function readProjectHistory(
  projectPath: string,
  fileName: string,
): Promise<unknown> {
  const content = await invoke<string>("read_project_history", { projectPath, fileName });
  return JSON.parse(content) as unknown;
}

export function saveImagePng(path: string, pngBase64: string): Promise<void> {
  return invoke("save_image_png", { path, pngBase64 });
}

export async function openProjectFile(path: string): Promise<unknown> {
  const content = await invoke<string>("open_project_file", { path });
  return JSON.parse(content) as unknown;
}

export interface ImportedSample {
  pngDataUrl: string;
  relativePath: string;
  width: number;
  height: number;
  sha256: string;
}

export function importSamplePng(
  projectPath: string | null,
  sourcePath: string,
  storage: "embedded" | "external",
): Promise<ImportedSample> {
  return invoke("import_sample_png", { projectPath, sourcePath, storage });
}

export function storeSamplePngData(
  projectPath: string | null,
  pngDataUrl: string,
  storage: "embedded" | "external",
): Promise<ImportedSample> {
  return invoke("store_sample_png_data", { projectPath, pngDataUrl, storage });
}

export function loadSamplePng(
  projectPath: string,
  relativePath: string,
  expectedSha256: string,
): Promise<ImportedSample> {
  return invoke("load_sample_png", { projectPath, relativePath, expectedSha256 });
}

export function listPngFiles(directory: string): Promise<string[]> {
  return invoke("list_png_files", { directory });
}

export interface SampleAudit {
  missing: string[];
  modified: string[];
  orphaned: string[];
}

export function auditProjectSamples(
  projectPath: string,
  references: { relativePath: string; sha256: string }[],
): Promise<SampleAudit> {
  return invoke("audit_project_samples", { projectPath, references });
}

export function cleanupOrphanSamples(
  projectPath: string,
  relativePaths: string[],
): Promise<number> {
  return invoke("cleanup_orphan_samples", { projectPath, relativePaths });
}

export function saveTextFile(path: string, content: string): Promise<void> {
  return invoke("save_text_file", { path, content });
}

export interface RuntimeDiagnostics {
  appVersion: string;
  operatingSystem: string;
  architecture: string;
  winsitterDllPresent: boolean;
  generatedAt: number;
}

export function runtimeDiagnostics(): Promise<RuntimeDiagnostics> {
  return invoke("runtime_diagnostics");
}

export function exportDiagnostics(path: string, projectSummary: object): Promise<void> {
  return invoke("export_diagnostics", { path, projectSummary });
}
