/**
 * 项目数据模型：与 Rust 匹配模型保持对应，JSON 字段使用 camelCase。
 */

export interface Region {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type AlphaMode = "ignore" | "match";

export interface FeaturePoint {
  dx: number;
  dy: number;
  reference: [number, number, number, number];
  tolerance: [number, number, number];
  alphaMode: AlphaMode;
  alphaTolerance: number;
  /** 排除点：必须不匹配才算通过。 */
  mustNot: boolean;
}

/** 一个可独立启停的视觉状态；对象的任意启用状态命中即代表对象出现。 */
export interface FeatureGroup {
  id: string;
  name: string;
  enabled: boolean;
  points: FeaturePoint[];
  /** 最少需要通过的常规点数；-1 表示全部。 */
  minMatch: number;
}

export interface ObjectSpec {
  id: string;
  name: string;
  region: Region;
  groups: FeatureGroup[];
  /** fixed=固定像素；scale=随客户区比例缩放；anchor=保持尺寸并按锚点移动。 */
  coordinateMode: "fixed" | "scale" | "anchor";
  anchorX: "start" | "center" | "end";
  anchorY: "start" | "center" | "end";
  /** 在预期位置周围搜索的半径，0～32 像素。 */
  searchRadius: number;
  /** 在适配后尺寸附近额外搜索的缩放百分比，0～10。 */
  scaleSearchPercent: number;
}

export interface TargetHint {
  windowTitle: string;
  className: string;
  processId: number;
  /** 标注时客户区基准尺寸；0 表示旧项目未记录。 */
  frameWidth: number;
  frameHeight: number;
  /** 标注时目标窗口 DPI；0 表示未知。 */
  baselineDpi: number;
}

export interface ReplayExpectation {
  objectId: string;
  /** null 表示该对象应不存在。 */
  expectedGroupId: string | null;
}

/** 一张离线回放帧可同时描述多个对象的预期状态。 */
export interface ReplayCase {
  id: string;
  name: string;
  capturedAt: number;
  width: number;
  height: number;
  storage: "embedded" | "external";
  pngDataUrl: string;
  /** 相对项目文件所在目录；仅 external 使用。 */
  relativePath: string;
  sha256: string;
  expectations: ReplayExpectation[];
  tags: string[];
}

export interface Project {
  version: number;
  target: TargetHint;
  objects: ObjectSpec[];
  replayCases: ReplayCase[];
}

export const CURRENT_PROJECT_VERSION = 4;

/** 后端匹配返回的逐点诊断。 */
export interface PointResult {
  index: number;
  ok: boolean;
  reason: string;
  actual: [number, number, number, number];
  delta: [number, number, number, number];
  /** 与参考色的直观相似度，0～100；不等同于容差判定。 */
  similarity: number;
  /** 超过容差最严重的通道及超出量；通过时为 0。 */
  maxExcess: number;
}

export interface GroupResult {
  id: string;
  matched: boolean;
  passedCount: number;
  required: number;
  points: PointResult[];
}

export interface MatchReport {
  matched: boolean;
  groups: GroupResult[];
  elapsedMicros: number;
  offsetX: number;
  offsetY: number;
  matchedScale: number;
}

export interface WindowItem {
  hwnd: number;
  title: string;
  className: string;
  processId: number;
  state: number;
  dpi: number;
}

export interface BoundTarget {
  targetId: number;
  hwnd: number;
  title: string;
  restoreWarning: string | null;
}

export interface FramePayload {
  pngDataUrl: string;
  width: number;
  height: number;
}

export interface ScreenOrigin {
  x: number;
  y: number;
}

export interface PointSuggestion {
  index: number;
  reference: [number, number, number, number];
  tolerance: [number, number, number];
  minObserved: [number, number, number, number];
  maxObserved: [number, number, number, number];
  alphaStable: boolean;
  alphaOpaque: boolean;
  suggestedAlphaMode: AlphaMode;
  alphaTolerance: number;
  alphaRange: number;
  negativeMatches: number;
  maxHalfRange: number;
  /** 综合正样本稳定性与负样本区分度的质量分，0～100。 */
  qualityScore: number;
  recommendKeep: boolean;
  qualityReason: string;
}

export interface FeatureCandidate {
  dx: number;
  dy: number;
  reference: [number, number, number, number];
  tolerance: [number, number, number];
  qualityScore: number;
  positiveRange: number;
  negativeDistance: number;
}

export function emptyProject(): Project {
  return {
    version: CURRENT_PROJECT_VERSION,
    target: {
      windowTitle: "",
      className: "",
      processId: 0,
      frameWidth: 0,
      frameHeight: 0,
      baselineDpi: 0,
    },
    objects: [],
    replayCases: [],
  };
}

export function defaultPoint(
  dx: number,
  dy: number,
  reference: [number, number, number, number] = [255, 0, 255, 255],
): FeaturePoint {
  return {
    dx,
    dy,
    reference,
    tolerance: [20, 20, 20],
    alphaMode: "ignore",
    alphaTolerance: 40,
    mustNot: false,
  };
}
