<script setup lang="ts">
/**
 * 捕获页：选择并绑定目标窗口，实时预览后台捕获画面，
 * 框选对象区域（支持整体拖拽/边框缩放/箭头微调）、
 * 像素级缩放、取色并添加特征点（支持键盘单像素移动）。
 */
import {
  CircleHelp,
  Copy,
  Crosshair,
  Loader2,
  MousePointer2,
  Pause,
  Pipette,
  Play,
  RefreshCw,
  Unlink,
} from "lucide-vue-next";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { toast } from "vue-sonner";
import { save } from "@tauri-apps/plugin-dialog";
import Button from "@/components/ui/button/Button.vue";
import Badge from "@/components/ui/badge/Badge.vue";
import Dialog from "@/components/ui/dialog/Dialog.vue";
import Input from "@/components/ui/input/Input.vue";
import Select from "@/components/ui/select/Select.vue";
import type { SelectOption } from "@/components/ui/select/Select.vue";
import Separator from "@/components/ui/separator/Separator.vue";
import { rgbaToHex } from "@/lib/utils";
import {
  relaunchElevated,
  restoreWindow,
  runMatchAdvanced,
  saveImagePng,
  targetScreenOrigin,
} from "@/lib/ipc";
import { frameSizeCompatible, resolveObjectForFrame } from "@/lib/matching";
import type { MatchReport, Region, WindowItem } from "@/lib/types";
import { useProjectStore } from "@/stores/project";
import { useTargetStore } from "@/stores/target";

const projectStore = useProjectStore();
const targetStore = useTargetStore();

type Mode = "region" | "pick";
const mode = ref<Mode>("region");

const canvasRef = ref<HTMLCanvasElement | null>(null);
const viewportRef = ref<HTMLDivElement | null>(null);
const wrapRef = ref<HTMLDivElement | null>(null);
const loupeRef = ref<HTMLCanvasElement | null>(null);
const rulerXRef = ref<HTMLCanvasElement | null>(null);
const rulerYRef = ref<HTMLCanvasElement | null>(null);
const guideInputRef = ref<HTMLInputElement | null>(null);
const pixelCanvas = document.createElement("canvas");
pixelCanvas.width = 1;
pixelCanvas.height = 1;
const pixelContext = pixelCanvas.getContext("2d", { willReadFrequently: true });

/** 画布相对视口的外边距（与模板 m-3 一致） */
const CANVAS_MARGIN = 12;
/** 标尺宽/高（px） */
const RULER_SIZE = 22;
/** 窗口状态：最小化（与后端 WINDOW_STATE_MINIMIZED 一致） */
const WINDOW_STATE_MINIMIZED = 2;

const ZOOM_STEPS = [0.1, 0.25, 0.5, 1, 2, 4, 8, 16];
const zoom = ref<string>("fit");
const zoomOptions: SelectOption[] = [
  { value: "fit", label: "适应窗口" },
  ...ZOOM_STEPS.map((value) => ({
    value: String(value),
    label: `${Math.round(value * 100)}%${value >= 8 ? "（像素级）" : ""}`,
  })),
];

const hoverPixel = ref<{
  x: number;
  y: number;
  rgba: [number, number, number, number];
} | null>(null);

/** 框选新区域的拖拽状态 */
const dragging = ref(false);
const dragStart = ref<{ x: number; y: number } | null>(null);
const dragCurrent = ref<{ x: number; y: number } | null>(null);

type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

/** 已有区域的整体移动/边框缩放拖拽状态 */
const boxDrag = ref<{
  kind: "move" | "resize";
  handle?: Handle;
  origin: Region;
  start: { x: number; y: number };
} | null>(null);
/** 拖拽中的实时区域预览 */
const liveRegion = ref<Region | null>(null);
/** 当前悬停在已有区域上的命中位置（用于光标提示） */
const hoverHandle = ref<Handle | "inside" | null>(null);

/** 取色模式的键盘光标位置 */
const pickCursor = ref<{ x: number; y: number } | null>(null);

/** Ctrl 是否按下（框选/取色需要 Ctrl，否则拖拽平移） */
const ctrlHeld = ref(false);

/** 画布平移拖拽状态 */
const pan = ref<{
  scrollLeft: number;
  scrollTop: number;
  startX: number;
  startY: number;
  moved: boolean;
  clickCoords: { x: number; y: number } | null;
} | null>(null);

/** 参考线：竖线的 x / 横线的 y（帧坐标，相对图片左上角 0,0） */
const guideX = ref<number | null>(null);
const guideY = ref<number | null>(null);
const guideDrag = ref<"x" | "y" | null>(null);
/** 双击参考线后的精确坐标输入框 */
const guideEditor = ref<{ axis: "x" | "y"; value: string } | null>(null);

/** 目标窗口客户区原点的屏幕坐标，用于把帧坐标换算为屏幕位置 */
const screenOrigin = ref<{ x: number; y: number } | null>(null);
let lastOriginRefresh = 0;

/** 视口滚动计数器：滚动位置不是响应式的，靠它驱动参考线/标尺重算 */
const scrollTick = ref(0);

/** 双击选区框边线后的坐标输入框：横线设 y，纵线设 x */
const edgeEditor = ref<{
  edge: "top" | "bottom" | "left" | "right";
  value: string;
  left: number;
  top: number;
} | null>(null);
const edgeInputRef = ref<HTMLInputElement | null>(null);

/** 画布右键菜单 */
const contextMenu = ref<{ x: number; y: number } | null>(null);

/** 选中特征组的实时匹配预览 */
const liveMatch = ref<MatchReport | null>(null);
let liveMatchTimer: ReturnType<typeof setTimeout> | null = null;
let liveMatchRunning = false;
let liveMatchRequested = false;
let liveMatchSequence = 0;

/** 帮助弹框 */
const helpOpen = ref(false);

/** 选区图像预览弹框（截图取自当前预览帧） */
const regionPreviewOpen = ref(false);
const regionPreviewUrl = ref<string | null>(null);

/** 复制坐标弹框：多规格坐标对照与一键复制 */
const coordsOpen = ref(false);

/** 待确认绑定的窗口（绑定前确认对话框） */
const pendingBind = ref<WindowItem | null>(null);

const viewportSize = ref({ w: 800, h: 600 });

function clamp(value: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, value));
}

const displayScale = computed(() => {
  const fw = targetStore.frameWidth || 1;
  const fh = targetStore.frameHeight || 1;
  let scale: number;
  if (zoom.value === "fit") {
    scale = Math.min(
      (viewportSize.value.w - 24) / fw,
      (viewportSize.value.h - 24) / fh,
    );
  } else {
    scale = Number(zoom.value);
  }
  // 画布硬上限保护：单边 16384px、总面积约 2.68 亿像素
  const maxByDim = 16384 / Math.max(fw, fh);
  const maxByArea = Math.sqrt(268435456 / (fw * fh));
  return Math.max(0.02, Math.min(scale, maxByDim, maxByArea));
});

const HANDLE_CURSOR: Record<Handle, string> = {
  nw: "nwse-resize",
  se: "nwse-resize",
  ne: "nesw-resize",
  sw: "nesw-resize",
  n: "ns-resize",
  s: "ns-resize",
  e: "ew-resize",
  w: "ew-resize",
};

const canvasCursor = computed(() => {
  if (pan.value) return "grabbing";
  if (boxDrag.value) {
    return boxDrag.value.kind === "move"
      ? "move"
      : HANDLE_CURSOR[boxDrag.value.handle ?? "e"];
  }
  if (mode.value === "region" && hoverHandle.value) {
    return hoverHandle.value === "inside"
      ? "move"
      : HANDLE_CURSOR[hoverHandle.value];
  }
  return ctrlHeld.value ? "crosshair" : "grab";
});

/** 竖参考线在视口中的横向位置（px）；不可见时为 null */
const guideVPos = computed<number | null>(() => {
  void scrollTick.value;
  const vp = viewportRef.value;
  if (guideX.value == null || !vp) return null;
  const pos = CANVAS_MARGIN + guideX.value * displayScale.value - vp.scrollLeft;
  return pos >= 0 && pos <= vp.clientWidth ? pos : null;
});

/** 横参考线在视口中的纵向位置（px）；不可见时为 null */
const guideHPos = computed<number | null>(() => {
  void scrollTick.value;
  const vp = viewportRef.value;
  if (guideY.value == null || !vp) return null;
  const pos = CANVAS_MARGIN + guideY.value * displayScale.value - vp.scrollTop;
  return pos >= 0 && pos <= vp.clientHeight ? pos : null;
});

/** 竖参考线坐标标签：默认相对图片左上角，括号内为屏幕位置 */
const guideXLabel = computed(() => {
  if (guideX.value == null) return "";
  const origin = screenOrigin.value;
  return origin
    ? `x=${guideX.value}（屏 ${origin.x + guideX.value}）`
    : `x=${guideX.value}`;
});

/** 横参考线坐标标签 */
const guideYLabel = computed(() => {
  if (guideY.value == null) return "";
  const origin = screenOrigin.value;
  return origin
    ? `y=${guideY.value}（屏 ${origin.y + guideY.value}）`
    : `y=${guideY.value}`;
});

/** 参考线坐标输入框的定位 */
const guideEditorStyle = computed(() => {
  const editor = guideEditor.value;
  if (!editor) return {};
  if (editor.axis === "x") {
    return { left: `${(guideVPos.value ?? 8) + 6}px`, top: "26px" };
  }
  return { left: "28px", top: `${(guideHPos.value ?? 8) - 12}px` };
});

/** 实时预览中当前选中特征组的结果 */
const liveGroupResult = computed(() => {
  const gid = projectStore.selectedGroupId;
  if (!liveMatch.value || !gid) return null;
  return liveMatch.value.groups.find((g) => g.id === gid) ?? null;
});

/** 选区左上/右下角坐标文本（右下角取包含式末像素） */
const regionCorners = computed(() => {
  const region = projectStore.selectedObject?.region;
  if (!region) return { topLeft: "-, -", bottomRight: "-, -" };
  return {
    topLeft: `${region.x}, ${region.y}`,
    bottomRight: `${region.x + region.w - 1}, ${region.y + region.h - 1}`,
  };
});

const objectOptions = computed<SelectOption[]>(() =>
  projectStore.project.objects.map((object) => ({
    value: object.id,
    label: object.name,
  })),
);

const groupOptions = computed<SelectOption[]>(() =>
  (projectStore.selectedObject?.groups ?? []).map((group) => ({
    value: group.id,
    label: group.name,
  })),
);

const projectFrameMismatch = computed(() => {
  const target = projectStore.project.target;
  return Boolean(
    targetStore.bound &&
      target.frameWidth > 0 &&
      target.frameHeight > 0 &&
      targetStore.frameWidth > 0 &&
      targetStore.frameHeight > 0 &&
      (target.frameWidth !== targetStore.frameWidth ||
        target.frameHeight !== targetStore.frameHeight),
  );
});

const coordinateMismatch = computed(
  () => !frameSizeCompatible(
    projectStore.selectedObject,
    projectStore.project.target.frameWidth,
    projectStore.project.target.frameHeight,
    targetStore.frameWidth,
    targetStore.frameHeight,
  ),
);
const editingFrameMismatch = computed(() => projectFrameMismatch.value);

function measureViewport() {
  const el = viewportRef.value;
  if (el) {
    viewportSize.value = { w: el.clientWidth, h: el.clientHeight };
  }
  void nextTick(drawRulers);
}

let resizeObserver: ResizeObserver | null = null;

onMounted(async () => {
  measureViewport();
  resizeObserver = new ResizeObserver(measureViewport);
  if (viewportRef.value) {
    resizeObserver.observe(viewportRef.value);
    // Ctrl + 滚轮缩放；需 passive:false 才能 preventDefault
    viewportRef.value.addEventListener("wheel", onWheel, { passive: false });
  }
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  if (!targetStore.bound) {
    await targetStore.refreshWindows();
  } else {
    targetStore.startPreview();
  }
});

onUnmounted(() => {
  resizeObserver?.disconnect();
  viewportRef.value?.removeEventListener("wheel", onWheel);
  window.removeEventListener("keydown", onKeyDown);
  window.removeEventListener("keyup", onKeyUp);
  endDragListeners();
  if (liveMatchTimer) clearTimeout(liveMatchTimer);
  liveMatchRequested = false;
  liveMatchSequence++;
  targetStore.stopPreview();
});

/** 帧或交互状态变化时重绘画布 */
watch(
  () => [
    targetStore.frameBitmap,
    displayScale.value,
    dragging.value,
    boxDrag.value,
    liveRegion.value,
    mode.value,
    pickCursor.value,
  ],
  () => {
    void nextTick(draw);
  },
);

/** 新预览帧到达后重新验证；与帧率解耦并防止 IPC 请求重叠。 */
watch(
  () => [targetStore.frameUpdatedAt, targetStore.bound?.targetId],
  () => {
    const target = projectStore.project.target;
    if (
      targetStore.frameWidth > 0 &&
      targetStore.frameHeight > 0 &&
      target.frameWidth === 0 &&
      target.frameHeight === 0
    ) {
      projectStore.setTargetFrameSize(
        targetStore.frameWidth,
        targetStore.frameHeight,
        targetStore.currentDpi,
      );
    }
    const bound = targetStore.bound;
    if (bound && Date.now() - lastOriginRefresh >= 1000) {
      lastOriginRefresh = Date.now();
      const targetId = bound.targetId;
      void targetScreenOrigin(targetId)
        .then((origin) => {
          if (targetStore.bound?.targetId === targetId) screenOrigin.value = origin;
        })
        .catch(() => {
          if (targetStore.bound?.targetId === targetId) screenOrigin.value = null;
        });
    }
    scheduleLiveMatch();
  },
);
/** 滚动/参考线/悬停变化时重画标尺（不动主画布，避免高倍缩放下的重绘开销） */
watch(
  () => [
    scrollTick.value,
    guideX.value,
    guideY.value,
    hoverPixel.value,
    displayScale.value,
    targetStore.frameBitmap,
    targetStore.bound,
  ],
  () => {
    void nextTick(drawRulers);
  },
);
watch(
  () => [
    projectStore.selectedObject?.region,
    projectStore.selectedGroup?.points,
    projectStore.selectedGroupId,
  ],
  () => {
    void nextTick(draw);
    scheduleLiveMatch();
  },
  { deep: true },
);

function draw() {
  const canvas = canvasRef.value;
  const bitmap = targetStore.frameBitmap;
  if (!canvas) return;
  const scale = displayScale.value;
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  if (!bitmap || !fw || !fh) {
    canvas.width = 0;
    canvas.height = 0;
    return;
  }
  const width = Math.max(1, Math.round(fw * scale));
  const height = Math.max(1, Math.round(fh * scale));
  if (canvas.width !== width) canvas.width = width;
  if (canvas.height !== height) canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.imageSmoothingEnabled = scale <= 1;
  ctx.clearRect(0, 0, width, height);
  ctx.drawImage(bitmap, 0, 0, width, height);

  // 像素级放大时叠加像素网格，便于精确选点
  if (scale >= 8) {
    ctx.strokeStyle = "rgba(255,255,255,0.12)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let gx = 0; gx <= fw; gx++) {
      ctx.moveTo(gx * scale + 0.5, 0);
      ctx.lineTo(gx * scale + 0.5, height);
    }
    for (let gy = 0; gy <= fh; gy++) {
      ctx.moveTo(0, gy * scale + 0.5);
      ctx.lineTo(width, gy * scale + 0.5);
    }
    ctx.stroke();
  }

  // 已定义的其他对象区域（浅色提示）
  for (const object of projectStore.project.objects) {
    if (object.id === projectStore.selectedObjectId) continue;
    const resolved = resolveObjectForFrame(
      object,
      projectStore.project.target.frameWidth,
      projectStore.project.target.frameHeight,
      fw,
      fh,
    );
    ctx.strokeStyle = "rgba(148, 163, 184, 0.7)";
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 3]);
    ctx.strokeRect(
      resolved.region.x * scale,
      resolved.region.y * scale,
      resolved.region.w * scale,
      resolved.region.h * scale,
    );
    ctx.setLineDash([]);
  }

  // 当前选中对象区域与缩放手柄
  const object = projectStore.selectedObject;
  if (object) {
    const resolved = resolveObjectForFrame(
      object,
      projectStore.project.target.frameWidth,
      projectStore.project.target.frameHeight,
      fw,
      fh,
    );
    const { x, y, w, h } = resolved.region;
    ctx.strokeStyle = "#22d3ee";
    ctx.lineWidth = Math.max(1, scale > 2 ? 2 : 1);
    ctx.strokeRect(x * scale, y * scale, w * scale, h * scale);

    const hs = 7;
    const handles = handlePositions(resolved.region);
    for (const [hx, hy] of handles) {
      ctx.fillStyle = "#ffffff";
      ctx.fillRect(hx * scale - hs / 2, hy * scale - hs / 2, hs, hs);
      ctx.strokeStyle = "#22d3ee";
      ctx.lineWidth = 1;
      ctx.strokeRect(hx * scale - hs / 2, hy * scale - hs / 2, hs, hs);
    }

    // 选中组的特征点标记（有实时匹配结果时按通过/失败着色）
    const group = resolved.groups.find((item) => item.id === projectStore.selectedGroupId);
    if (group) {
      const liveGroup =
        liveMatch.value?.groups.find((g) => g.id === group.id) ?? null;
      for (const [index, point] of group.points.entries()) {
        const px = (x + point.dx) * scale;
        const py = (y + point.dy) * scale;
        const livePoint = liveGroup?.points.find((p) => p.index === index);
        if (livePoint) {
          const radius = Math.max(7, Math.min(24, 7 + livePoint.maxExcess / 5));
          const gradient = ctx.createRadialGradient(px, py, 1, px, py, radius);
          gradient.addColorStop(
            0,
            livePoint.ok ? "rgba(74,222,128,0.5)" : "rgba(248,113,113,0.65)",
          );
          gradient.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = gradient;
          ctx.beginPath();
          ctx.arc(px, py, radius, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.fillStyle = livePoint
          ? livePoint.ok
            ? "#4ade80"
            : "#f87171"
          : point.mustNot
            ? "#f87171"
            : "#4ade80";
        ctx.strokeStyle = "rgba(0,0,0,0.8)";
        ctx.lineWidth = 1;
        const size = Math.max(3, scale * 0.8);
        ctx.fillRect(px - size / 2, py - size / 2, size, size);
        ctx.strokeRect(px - size / 2, py - size / 2, size, size);
        if (scale >= 4) {
          ctx.font = "10px sans-serif";
          ctx.fillStyle = "rgba(0,0,0,0.85)";
          ctx.fillRect(px + 4, py - 12, 18, 12);
          ctx.fillStyle = "white";
          ctx.fillText(String(index + 1), px + 7, py - 3);
        }
      }
    }
  }

  // 拖拽中的新区域（框选）
  if (dragging.value && dragStart.value && dragCurrent.value) {
    const left = Math.min(dragStart.value.x, dragCurrent.value.x) * scale;
    const top = Math.min(dragStart.value.y, dragCurrent.value.y) * scale;
    const w = Math.abs(dragCurrent.value.x - dragStart.value.x) * scale;
    const h = Math.abs(dragCurrent.value.y - dragStart.value.y) * scale;
    ctx.fillStyle = "rgba(34, 211, 238, 0.12)";
    ctx.fillRect(left, top, w, h);
    ctx.strokeStyle = "#22d3ee";
    ctx.setLineDash([6, 3]);
    ctx.strokeRect(left, top, w, h);
    ctx.setLineDash([]);
  }

  // 已有区域的移动/缩放预览
  const live = liveRegion.value;
  if (boxDrag.value && live) {
    ctx.fillStyle = "rgba(34, 211, 238, 0.12)";
    ctx.fillRect(live.x * scale, live.y * scale, live.w * scale, live.h * scale);
    ctx.strokeStyle = "#fbbf24";
    ctx.setLineDash([6, 3]);
    ctx.lineWidth = 2;
    ctx.strokeRect(live.x * scale, live.y * scale, live.w * scale, live.h * scale);
    ctx.setLineDash([]);
  }

  // 取色模式键盘光标
  const cursor = pickCursor.value;
  if (mode.value === "pick" && cursor) {
    const cx = cursor.x * scale + scale / 2;
    const cy = cursor.y * scale + scale / 2;
    ctx.strokeStyle = "rgba(34, 211, 238, 0.9)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(cx, 0);
    ctx.lineTo(cx, height);
    ctx.moveTo(0, cy);
    ctx.lineTo(width, cy);
    ctx.stroke();
    ctx.strokeStyle = "#fbbf24";
    ctx.lineWidth = 2;
    ctx.strokeRect(cursor.x * scale, cursor.y * scale, scale, scale);
  }
}

/** 区域的 8 个手柄锚点（帧坐标） */
function handlePositions(region: Region): [number, number][] {
  const { x, y, w, h } = region;
  const right = x + w - 1;
  const bottom = y + h - 1;
  const midX = x + (w - 1) / 2;
  const midY = y + (h - 1) / 2;
  return [
    [x, y],
    [midX, y],
    [right, y],
    [right, midY],
    [right, bottom],
    [midX, bottom],
    [x, bottom],
    [x, midY],
  ];
}

/** 把鼠标事件换算为帧坐标；越界返回 null */
function toFrameCoords(event: MouseEvent): { x: number; y: number } | null {
  const canvas = canvasRef.value;
  if (!canvas) return null;
  const rect = canvas.getBoundingClientRect();
  const scale = displayScale.value;
  const x = Math.floor((event.clientX - rect.left) / scale);
  const y = Math.floor((event.clientY - rect.top) / scale);
  if (
    x < 0 ||
    y < 0 ||
    x >= targetStore.frameWidth ||
    y >= targetStore.frameHeight
  ) {
    return null;
  }
  return { x, y };
}

/** 把鼠标事件换算为帧坐标；越界时夹紧到帧内（拖拽中允许移出画布） */
function toFrameCoordsClamped(event: MouseEvent): { x: number; y: number } | null {
  const canvas = canvasRef.value;
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  if (!canvas || !fw || !fh) return null;
  const rect = canvas.getBoundingClientRect();
  const scale = displayScale.value;
  const x = clamp(Math.floor((event.clientX - rect.left) / scale), 0, fw - 1);
  const y = clamp(Math.floor((event.clientY - rect.top) / scale), 0, fh - 1);
  return { x, y };
}

function readPixel(x: number, y: number): [number, number, number, number] | null {
  const bitmap = targetStore.frameBitmap;
  if (!bitmap || !pixelContext) return null;
  pixelContext.clearRect(0, 0, 1, 1);
  pixelContext.drawImage(bitmap, x, y, 1, 1, 0, 0, 1, 1);
  const data = pixelContext.getImageData(0, 0, 1, 1).data;
  return [data[0], data[1], data[2], data[3]];
}

function updateHover(x: number, y: number) {
  const rgba = readPixel(x, y);
  if (rgba) {
    hoverPixel.value = { x, y, rgba };
    updateLoupe(x, y);
  }
}

function updateLoupe(x: number, y: number) {
  const loupe = loupeRef.value;
  const bitmap = targetStore.frameBitmap;
  if (!loupe) return;
  const ctx = loupe.getContext("2d");
  if (!ctx) return;
  const size = 160;
  loupe.width = size;
  loupe.height = size;
  ctx.imageSmoothingEnabled = false;
  ctx.fillStyle = "#1e293b";
  ctx.fillRect(0, 0, size, size);
  if (bitmap) {
    const span = 11;
    const half = Math.floor(span / 2);
    ctx.drawImage(bitmap, x - half, y - half, span, span, 0, 0, size, size);
    // 中心像素网格
    const cell = size / span;
    ctx.strokeStyle = "rgba(255,255,255,0.25)";
    ctx.lineWidth = 1;
    for (let i = 1; i < span; i++) {
      ctx.beginPath();
      ctx.moveTo(i * cell, 0);
      ctx.lineTo(i * cell, size);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(0, i * cell);
      ctx.lineTo(size, i * cell);
      ctx.stroke();
    }
    ctx.strokeStyle = "#22d3ee";
    ctx.lineWidth = 2;
    ctx.strokeRect(half * cell, half * cell, cell, cell);
  }
}

/** 命中检测：返回悬停位置对应的手柄、区域内部或 null */
function hitTest(coords: { x: number; y: number }): Handle | "inside" | null {
  if (mode.value !== "region") return null;
  const object = projectStore.selectedObject;
  if (!object) return null;
  const { x, y, w, h } = object.region;
  const tol = Math.max(1, 5 / displayScale.value);
  const { x: fx, y: fy } = coords;
  const right = x + w - 1;
  const bottom = y + h - 1;
  const nearL = Math.abs(fx - x) <= tol;
  const nearR = Math.abs(fx - right) <= tol;
  const nearT = Math.abs(fy - y) <= tol;
  const nearB = Math.abs(fy - bottom) <= tol;
  const inX = fx >= x - tol && fx <= right + tol;
  const inY = fy >= y - tol && fy <= bottom + tol;
  if (nearL && nearT) return "nw";
  if (nearR && nearT) return "ne";
  if (nearR && nearB) return "se";
  if (nearL && nearB) return "sw";
  if (nearT && inX) return "n";
  if (nearB && inX) return "s";
  if (nearL && inY) return "w";
  if (nearR && inY) return "e";
  if (fx >= x && fx <= right && fy >= y && fy <= bottom) return "inside";
  return null;
}

/** 依据拖拽状态计算实时区域预览 */
function computeLiveRegion(
  drag: NonNullable<typeof boxDrag.value>,
  coords: { x: number; y: number },
): Region {
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  const { origin, start } = drag;
  if (drag.kind === "move") {
    const dx = coords.x - start.x;
    const dy = coords.y - start.y;
    return {
      x: clamp(origin.x + dx, 0, Math.max(0, fw - origin.w)),
      y: clamp(origin.y + dy, 0, Math.max(0, fh - origin.h)),
      w: origin.w,
      h: origin.h,
    };
  }
  let left = origin.x;
  let top = origin.y;
  let right = origin.x + origin.w;
  let bottom = origin.y + origin.h;
  const handle = drag.handle ?? "e";
  if (handle.includes("w")) left = clamp(coords.x, 0, right - 1);
  if (handle.includes("e")) right = clamp(coords.x + 1, left + 1, fw);
  if (handle.includes("n")) top = clamp(coords.y, 0, bottom - 1);
  if (handle.includes("s")) bottom = clamp(coords.y + 1, top + 1, fh);
  return { x: left, y: top, w: right - left, h: bottom - top };
}

function onMouseMove(event: MouseEvent) {
  const coords = toFrameCoords(event);
  if (!coords) {
    hoverPixel.value = null;
    hoverHandle.value = null;
    return;
  }
  updateHover(coords.x, coords.y);
  hoverHandle.value = boxDrag.value ? null : hitTest(coords);
}

function onMouseDown(event: MouseEvent) {
  contextMenu.value = null;
  // 中键任意模式下平移
  if (event.button === 1) {
    event.preventDefault();
    startPan(event);
    return;
  }
  if (event.button !== 0) return;
  const coords = toFrameCoords(event);
  if (!coords) return;
  if (editingFrameMismatch.value) {
    startPan(event);
    return;
  }
  if (mode.value === "region") {
    const object = projectStore.selectedObject;
    const hit = hitTest(coords);
    if (object && hit && hit !== "inside") {
      // 拖拽边框/角点调整大小
      boxDrag.value = {
        kind: "resize",
        handle: hit,
        origin: { ...object.region },
        start: coords,
      };
      liveRegion.value = { ...object.region };
      startDragListeners();
      return;
    }
    if (object && hit === "inside") {
      // 整体拖拽移动区域
      boxDrag.value = { kind: "move", origin: { ...object.region }, start: coords };
      liveRegion.value = { ...object.region };
      startDragListeners();
      return;
    }
    if (event.ctrlKey) {
      // Ctrl + 空白处拖拽才框选新区域
      dragging.value = true;
      dragStart.value = coords;
      dragCurrent.value = coords;
      startDragListeners();
    } else {
      startPan(event);
    }
  } else if (event.ctrlKey) {
    // 取色需要 Ctrl
    pickCursor.value = coords;
    updateHover(coords.x, coords.y);
    pickColor(coords.x, coords.y);
  } else {
    // 非 Ctrl 拖拽平移；未发生位移时仅移动取色光标不加点
    startPan(event);
  }
}

function startPan(event: MouseEvent) {
  const vp = viewportRef.value;
  if (!vp) return;
  pan.value = {
    scrollLeft: vp.scrollLeft,
    scrollTop: vp.scrollTop,
    startX: event.clientX,
    startY: event.clientY,
    moved: false,
    clickCoords: mode.value === "pick" ? toFrameCoords(event) : null,
  };
  startDragListeners();
}

function startDragListeners() {
  window.addEventListener("mousemove", onDragMove);
  window.addEventListener("mouseup", onDragUp);
}

function endDragListeners() {
  window.removeEventListener("mousemove", onDragMove);
  window.removeEventListener("mouseup", onDragUp);
}

function onDragMove(event: MouseEvent) {
  if (guideDrag.value === "x") {
    setGuideXFromEvent(event);
    return;
  }
  if (guideDrag.value === "y") {
    setGuideYFromEvent(event);
    return;
  }
  if (pan.value) {
    const vp = viewportRef.value;
    if (vp) {
      vp.scrollLeft = pan.value.scrollLeft - (event.clientX - pan.value.startX);
      vp.scrollTop = pan.value.scrollTop - (event.clientY - pan.value.startY);
      scrollTick.value++;
      if (
        Math.abs(event.clientX - pan.value.startX) +
          Math.abs(event.clientY - pan.value.startY) >
        4
      ) {
        pan.value.moved = true;
      }
    }
    return;
  }
  const coords = toFrameCoordsClamped(event);
  if (!coords) return;
  updateHover(coords.x, coords.y);
  if (dragging.value) {
    dragCurrent.value = coords;
  }
  if (boxDrag.value) {
    liveRegion.value = computeLiveRegion(boxDrag.value, coords);
  }
}

function onDragUp() {
  endDragListeners();
  if (guideDrag.value) {
    guideDrag.value = null;
    return;
  }
  if (pan.value) {
    const { moved, clickCoords } = pan.value;
    pan.value = null;
    // 取色模式下未位移的点击：只移动光标不加点
    if (!moved && clickCoords && mode.value === "pick") {
      pickCursor.value = clickCoords;
      updateHover(clickCoords.x, clickCoords.y);
    }
    return;
  }
  if (boxDrag.value) {
    const live = liveRegion.value;
    boxDrag.value = null;
    liveRegion.value = null;
    const object = projectStore.selectedObject;
    if (!object || !live) return;
    if (live.w < 1 || live.h < 1) return;
    if (projectStore.setRegion(object.id, live)) {
      toast.success(`区域已更新：(${live.x}, ${live.y}) ${live.w}×${live.h}`);
    } else {
      toast.error("区域不能缩小到已有特征点之外，请先移动或删除相关点");
    }
    return;
  }
  if (!dragging.value) return;
  dragging.value = false;
  const start = dragStart.value;
  const end = dragCurrent.value;
  dragStart.value = null;
  dragCurrent.value = null;
  if (!start || !end) return;
  const x = Math.min(start.x, end.x);
  const y = Math.min(start.y, end.y);
  const w = Math.abs(end.x - start.x) + 1;
  const h = Math.abs(end.y - start.y) + 1;
  if (w < 2 || h < 2) return;
  const object = projectStore.selectedObject;
  if (!object) {
    toast.error("请先在右侧新建或选择一个对象");
    return;
  }
  if (projectStore.setRegion(object.id, { x, y, w, h })) {
    toast.success(`区域已更新：(${x}, ${y}) ${w}×${h}`);
  } else {
    toast.error("新区域不能排除已有特征点，请先移动或删除相关点");
  }
}

/** 方向键微调区域整体位置；Shift 步进 10px */
function nudgeRegion(dx: number, dy: number) {
  const object = projectStore.selectedObject;
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  if (!object || !fw || !fh) return;
  const region = object.region;
  const x = clamp(region.x + dx, 0, Math.max(0, fw - region.w));
  const y = clamp(region.y + dy, 0, Math.max(0, fh - region.h));
  if (x !== region.x || y !== region.y) {
    projectStore.setRegion(object.id, { ...region, x, y });
  }
}

/** 方向键移动取色光标；Shift 步进 10px */
function movePickCursor(dx: number, dy: number) {
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  if (!fw || !fh) return;
  const cursor = pickCursor.value ?? {
    x: Math.floor(fw / 2),
    y: Math.floor(fh / 2),
  };
  const x = clamp(cursor.x + dx, 0, fw - 1);
  const y = clamp(cursor.y + dy, 0, fh - 1);
  pickCursor.value = { x, y };
  updateHover(x, y);
}

function onKeyDown(event: KeyboardEvent) {
  if (event.key === "Control") {
    ctrlHeld.value = true;
  }
  const target = event.target as HTMLElement | null;
  if (
    target &&
    ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)
  ) {
    return;
  }
  const step = event.shiftKey ? 10 : 1;
  const arrows: Record<string, [number, number]> = {
    ArrowLeft: [-step, 0],
    ArrowRight: [step, 0],
    ArrowUp: [0, -step],
    ArrowDown: [0, step],
  };
  if (event.key in arrows) {
    if (!targetStore.bound) return;
    event.preventDefault();
    const [dx, dy] = arrows[event.key];
    if (mode.value === "region") {
      nudgeRegion(dx, dy);
    } else {
      movePickCursor(dx, dy);
    }
    return;
  }
  if ((event.key === "Enter" || event.key === " ") && mode.value === "pick") {
    event.preventDefault();
    const cursor = pickCursor.value;
    if (cursor) pickColor(cursor.x, cursor.y);
  }
}

function onKeyUp(event: KeyboardEvent) {
  if (event.key === "Control") ctrlHeld.value = false;
}

/** Ctrl + 滚轮在预设档位间缩放，并保持视口中心像素不动 */
let zoomAnchor: { fx: number; fy: number } | null = null;

function setZoom(value: string) {
  const vp = viewportRef.value;
  if (vp) {
    const scale = displayScale.value;
    zoomAnchor = {
      fx: (vp.scrollLeft + vp.clientWidth / 2 - CANVAS_MARGIN) / scale,
      fy: (vp.scrollTop + vp.clientHeight / 2 - CANVAS_MARGIN) / scale,
    };
  }
  zoom.value = value;
}

watch(zoom, () => {
  void nextTick(() => {
    const vp = viewportRef.value;
    if (!vp || !zoomAnchor) return;
    const scale = displayScale.value;
    vp.scrollLeft = CANVAS_MARGIN + zoomAnchor.fx * scale - vp.clientWidth / 2;
    vp.scrollTop = CANVAS_MARGIN + zoomAnchor.fy * scale - vp.clientHeight / 2;
    zoomAnchor = null;
  });
});

function onWheel(event: WheelEvent) {
  if (!event.ctrlKey || !targetStore.bound) return;
  event.preventDefault();
  // 以当前实际显示比例为基准找最近档位，避免从适应档突变
  const current =
    zoom.value === "fit" ? displayScale.value : Number(zoom.value);
  let index = 0;
  let best = Number.POSITIVE_INFINITY;
  ZOOM_STEPS.forEach((step, i) => {
    const distance = Math.abs(Math.log(step) - Math.log(current));
    if (distance < best - 1e-9) {
      best = distance;
      index = i;
    }
  });
  const next = event.deltaY < 0 ? index + 1 : index - 1;
  const clamped = clamp(next, 0, ZOOM_STEPS.length - 1);
  const target = ZOOM_STEPS[clamped];
  if (Math.abs(Math.log(target) - Math.log(current)) > 1e-6) {
    setZoom(String(target));
  }
}

// ============ 标尺与参考线 ============

/** 读取 CSS 主题变量为颜色（变量已是完整 oklch/hsl 颜色，直接返回） */
function cssColor(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
  return value || fallback;
}

/** 选择标尺主刻度步长，保证刻度间距至少 60px */
function rulerStep(scale: number): number {
  const steps = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000];
  for (const step of steps) {
    if (step * scale >= 60) return step;
  }
  return steps[steps.length - 1];
}

/** 绘制顶部/左侧标尺：刻度、悬停十字与参考线位置同步 */
function drawRulers() {
  const vp = viewportRef.value;
  const rx = rulerXRef.value;
  const ry = rulerYRef.value;
  if (!vp || !rx || !ry) return;
  const scale = displayScale.value;
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  const border = cssColor("--border", "#64748b");
  const text = cssColor("--muted-foreground", "#94a3b8");
  const bg = cssColor("--background", "#0f172a");

  // 横向标尺（x 坐标）
  const w = Math.max(1, vp.clientWidth);
  if (rx.width !== w) rx.width = w;
  if (rx.height !== RULER_SIZE) rx.height = RULER_SIZE;
  const ctxX = rx.getContext("2d");
  if (ctxX) {
    ctxX.fillStyle = bg;
    ctxX.fillRect(0, 0, w, RULER_SIZE);
    if (fw && fh) {
      const step = rulerStep(scale);
      const minor = step / 5;
      const first = Math.max(
        0,
        Math.floor((vp.scrollLeft - CANVAS_MARGIN) / scale / minor) * minor,
      );
      const last = Math.min(
        fw,
        Math.ceil((vp.scrollLeft + w - CANVAS_MARGIN) / scale / minor) * minor,
      );
      ctxX.strokeStyle = border;
      ctxX.fillStyle = text;
      ctxX.font = "9px sans-serif";
      ctxX.beginPath();
      for (let f = first; f <= last; f += minor) {
        const x = Math.round(CANVAS_MARGIN + f * scale - vp.scrollLeft) + 0.5;
        const major = f % step === 0;
        ctxX.moveTo(x, RULER_SIZE);
        ctxX.lineTo(x, RULER_SIZE - (major ? 8 : 4));
        if (major) ctxX.fillText(String(f), x + 2, 9);
      }
      ctxX.stroke();
      if (hoverPixel.value) {
        const x =
          Math.round(CANVAS_MARGIN + hoverPixel.value.x * scale - vp.scrollLeft) + 0.5;
        ctxX.strokeStyle = "#22d3ee";
        ctxX.beginPath();
        ctxX.moveTo(x, 0);
        ctxX.lineTo(x, RULER_SIZE);
        ctxX.stroke();
      }
      if (guideX.value != null) {
        const x =
          Math.round(CANVAS_MARGIN + guideX.value * scale - vp.scrollLeft) + 0.5;
        ctxX.strokeStyle = "#fbbf24";
        ctxX.beginPath();
        ctxX.moveTo(x, 0);
        ctxX.lineTo(x, RULER_SIZE);
        ctxX.stroke();
      }
    }
    ctxX.strokeStyle = border;
    ctxX.beginPath();
    ctxX.moveTo(0, RULER_SIZE - 0.5);
    ctxX.lineTo(w, RULER_SIZE - 0.5);
    ctxX.stroke();
  }

  // 纵向标尺（y 坐标）
  const h = Math.max(1, vp.clientHeight);
  if (ry.height !== h) ry.height = h;
  if (ry.width !== RULER_SIZE) ry.width = RULER_SIZE;
  const ctxY = ry.getContext("2d");
  if (ctxY) {
    ctxY.fillStyle = bg;
    ctxY.fillRect(0, 0, RULER_SIZE, h);
    if (fw && fh) {
      const step = rulerStep(scale);
      const minor = step / 5;
      const first = Math.max(
        0,
        Math.floor((vp.scrollTop - CANVAS_MARGIN) / scale / minor) * minor,
      );
      const last = Math.min(
        fh,
        Math.ceil((vp.scrollTop + h - CANVAS_MARGIN) / scale / minor) * minor,
      );
      ctxY.strokeStyle = border;
      ctxY.fillStyle = text;
      ctxY.font = "9px sans-serif";
      ctxY.beginPath();
      for (let f = first; f <= last; f += minor) {
        const y = Math.round(CANVAS_MARGIN + f * scale - vp.scrollTop) + 0.5;
        const major = f % step === 0;
        ctxY.moveTo(RULER_SIZE, y);
        ctxY.lineTo(RULER_SIZE - (major ? 8 : 4), y);
        if (major) {
          ctxY.save();
          ctxY.translate(9, y + 2);
          ctxY.rotate(-Math.PI / 2);
          ctxY.fillText(String(f), 0, 0);
          ctxY.restore();
        }
      }
      ctxY.stroke();
      if (hoverPixel.value) {
        const y =
          Math.round(CANVAS_MARGIN + hoverPixel.value.y * scale - vp.scrollTop) + 0.5;
        ctxY.strokeStyle = "#22d3ee";
        ctxY.beginPath();
        ctxY.moveTo(0, y);
        ctxY.lineTo(RULER_SIZE, y);
        ctxY.stroke();
      }
      if (guideY.value != null) {
        const y =
          Math.round(CANVAS_MARGIN + guideY.value * scale - vp.scrollTop) + 0.5;
        ctxY.strokeStyle = "#fbbf24";
        ctxY.beginPath();
        ctxY.moveTo(0, y);
        ctxY.lineTo(RULER_SIZE, y);
        ctxY.stroke();
      }
    }
    ctxY.strokeStyle = border;
    ctxY.beginPath();
    ctxY.moveTo(RULER_SIZE - 0.5, 0);
    ctxY.lineTo(RULER_SIZE - 0.5, h);
    ctxY.stroke();
  }
}

function setGuideXFromEvent(event: MouseEvent) {
  const vp = viewportRef.value;
  const fw = targetStore.frameWidth;
  if (!vp || !fw) return;
  const rect = vp.getBoundingClientRect();
  const fx = Math.round(
    (event.clientX - rect.left + vp.scrollLeft - CANVAS_MARGIN) / displayScale.value,
  );
  guideX.value = clamp(fx, 0, fw - 1);
  scrollTick.value++;
}

function setGuideYFromEvent(event: MouseEvent) {
  const vp = viewportRef.value;
  const fh = targetStore.frameHeight;
  if (!vp || !fh) return;
  const rect = vp.getBoundingClientRect();
  const fy = Math.round(
    (event.clientY - rect.top + vp.scrollTop - CANVAS_MARGIN) / displayScale.value,
  );
  guideY.value = clamp(fy, 0, fh - 1);
  scrollTick.value++;
}

/** 在标尺上按下拖动：创建或移动对应参考线 */
function onRulerMouseDown(axis: "x" | "y", event: MouseEvent) {
  if (!targetStore.bound) return;
  event.preventDefault();
  if (axis === "x") setGuideXFromEvent(event);
  else setGuideYFromEvent(event);
  guideDrag.value = axis;
  startDragListeners();
}

function onGuideMouseDown(axis: "x" | "y", event: MouseEvent) {
  event.preventDefault();
  event.stopPropagation();
  guideDrag.value = axis;
  startDragListeners();
}

/** 双击参考线：打开精确坐标输入框 */
function openGuideEditor(axis: "x" | "y") {
  const value = axis === "x" ? guideX.value : guideY.value;
  if (value == null) return;
  guideEditor.value = { axis, value: String(value) };
  void nextTick(() => {
    guideInputRef.value?.focus();
    guideInputRef.value?.select();
  });
}

function commitGuideEditor() {
  const editor = guideEditor.value;
  guideEditor.value = null;
  if (!editor) return;
  const value = Math.round(Number(editor.value));
  if (Number.isNaN(value)) return;
  if (editor.axis === "x") {
    guideX.value = clamp(value, 0, Math.max(0, targetStore.frameWidth - 1));
  } else {
    guideY.value = clamp(value, 0, Math.max(0, targetStore.frameHeight - 1));
  }
  scrollTick.value++;
}

function onViewportScroll() {
  scrollTick.value++;
  drawRulers();
}

// ============ 双击选区框边线设置坐标 ============

/** 双击选区框边线：横线设 y（top/bottom），纵线设 x（left/right） */
function onCanvasDblClick(event: MouseEvent) {
  contextMenu.value = null;
  if (!targetStore.bound || mode.value !== "region") return;
  const object = projectStore.selectedObject;
  if (!object) return;
  const point = toFrameCoords(event);
  if (!point) return;
  const { x, y, w, h } = object.region;
  const tol = Math.max(2, 6 / displayScale.value);
  const inX = point.x >= x - tol && point.x <= x + w - 1 + tol;
  const inY = point.y >= y - tol && point.y <= y + h - 1 + tol;
  let edge: "top" | "bottom" | "left" | "right" | null = null;
  let value = 0;
  if (Math.abs(point.y - y) <= tol && inX) {
    edge = "top";
    value = y;
  } else if (Math.abs(point.y - (y + h - 1)) <= tol && inX) {
    edge = "bottom";
    value = y + h - 1;
  } else if (Math.abs(point.x - x) <= tol && inY) {
    edge = "left";
    value = x;
  } else if (Math.abs(point.x - (x + w - 1)) <= tol && inY) {
    edge = "right";
    value = x + w - 1;
  }
  if (!edge) return;
  const wrap = wrapRef.value;
  const left = wrap ? event.clientX - wrap.getBoundingClientRect().left : event.clientX;
  const top = wrap ? event.clientY - wrap.getBoundingClientRect().top : event.clientY;
  edgeEditor.value = { edge, value: String(value), left, top };
  void nextTick(() => {
    edgeInputRef.value?.focus();
    edgeInputRef.value?.select();
  });
}

/** 提交边线坐标：按边调整选区 */
function commitEdgeEditor() {
  const editor = edgeEditor.value;
  const object = projectStore.selectedObject;
  if (!editor || !object) {
    edgeEditor.value = null;
    return;
  }
  const input = Number.parseInt(editor.value, 10);
  if (!Number.isNaN(input)) {
    const { x, y, w, h } = object.region;
    const fw = targetStore.frameWidth;
    const fh = targetStore.frameHeight;
    if (editor.edge === "top") {
      const top = clamp(input, 0, y + h - 1);
      projectStore.setRegion(object.id, { x, y: top, w, h: y + h - top });
    } else if (editor.edge === "bottom") {
      const bottom = clamp(input, y, fh - 1);
      projectStore.setRegion(object.id, { x, y, w, h: bottom - y + 1 });
    } else if (editor.edge === "left") {
      const left = clamp(input, 0, x + w - 1);
      projectStore.setRegion(object.id, { x: left, y, w: x + w - left, h });
    } else {
      const right = clamp(input, x, fw - 1);
      projectStore.setRegion(object.id, { x, y, w: right - x + 1, h });
    }
  }
  edgeEditor.value = null;
}

/** 边线输入框按键：Enter 提交，Esc 取消 */
function onEdgeEditorKeydown(event: KeyboardEvent) {
  event.stopPropagation();
  if (event.key === "Enter") {
    event.preventDefault();
    commitEdgeEditor();
  } else if (event.key === "Escape") {
    event.preventDefault();
    edgeEditor.value = null;
  }
}

// ============ 右键菜单：复制/保存选区图像 ============

/** 画布右键：在对象选区上弹出自定义菜单 */
function onCanvasContextMenu(event: MouseEvent) {
  if (!targetStore.bound || !projectStore.selectedObject) return;
  event.preventDefault();
  const wrap = wrapRef.value;
  const rect = wrap?.getBoundingClientRect();
  contextMenu.value = {
    x: rect ? event.clientX - rect.left : event.clientX,
    y: rect ? event.clientY - rect.top : event.clientY,
  };
}

/** 裁剪选区为 canvas（从原始帧位图按 1:1 裁剪，避免受显示缩放与像素网格影响） */
function regionToCanvas(): HTMLCanvasElement | null {
  const object = projectStore.selectedObject;
  const bitmap = targetStore.frameBitmap;
  if (!object || !bitmap) return null;
  const { x, y, w, h } = object.region;
  if (w <= 0 || h <= 0) return null;
  const cut = document.createElement("canvas");
  cut.width = w;
  cut.height = h;
  const ctx = cut.getContext("2d");
  if (!ctx) return null;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(bitmap, x, y, w, h, 0, 0, w, h);
  return cut;
}

/** 预览选区图像：把当前帧的裁剪结果弹窗展示，直观确认框选内容 */
function previewRegionImage() {
  contextMenu.value = null;
  const cut = regionToCanvas();
  if (!cut) {
    toast.error("当前选区无效");
    return;
  }
  regionPreviewUrl.value = cut.toDataURL("image/png");
  regionPreviewOpen.value = true;
}

/** 各规格坐标条目（均基于客户区坐标系） */
const coordFormats = computed(() => {
  const r = projectStore.selectedObject?.region;
  if (!r) return [];
  const right = r.x + r.w;
  const bottom = r.y + r.h;
  return [
    {
      name: "LTRB 左闭右开（winsitter 同款）",
      desc: "right = left + 宽、bottom = top + 高，右/下边界不含该列/行。与 winsitter RectU32、本项目导出 JSON 口径一致，区域截图裁剪直接用。",
      value: JSON.stringify({ left: r.x, top: r.y, right, bottom }),
    },
    {
      name: "x, y, w, h（OpenCV 风格）",
      desc: "原点 + 尺寸；末像素列 = x + w - 1，OpenCV Rect、多数游戏脚本框架常用。",
      value: JSON.stringify({ x: r.x, y: r.y, w: r.w, h: r.h }),
    },
    {
      name: "双闭角点（左上 → 右下）",
      desc: "右下角是包含在内的最后一个像素（本工具界面显示口径）；宽 = 右下 - 左上 + 1。",
      value: `(${r.x}, ${r.y}) -> (${right - 1}, ${bottom - 1})`,
    },
    {
      name: "PIL crop 调用",
      desc: "PIL 的 box 也是左闭右开，可直接裁剪客户区截图：裁出的图与本工具选区预览逐像素一致。",
      value: `frame.crop((${r.x}, ${r.y}, ${right}, ${bottom}))`,
    },
    {
      name: "NumPy 切片",
      desc: "数组切片同样左闭右开，注意先行后列：[top:bottom, left:right]。",
      value: `frame[${r.y}:${bottom}, ${r.x}:${right}]`,
    },
  ];
});

/** 打开复制坐标弹框 */
function openCopyCoords() {
  contextMenu.value = null;
  if (!projectStore.selectedObject) {
    toast.error("当前没有选中对象");
    return;
  }
  coordsOpen.value = true;
}

/** 复制单条坐标文本 */
async function copyCoordValue(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    toast.success("已复制到剪贴板");
  } catch (error) {
    toast.error(`复制失败：${error}`);
  }
}

/** 复制选区图像到剪贴板 */
async function copyRegionImage() {
  contextMenu.value = null;
  const cut = regionToCanvas();
  if (!cut) {
    toast.error("当前选区无效");
    return;
  }
  const blob = await new Promise<Blob | null>((resolve) =>
    cut.toBlob(resolve, "image/png"),
  );
  if (!blob) {
    toast.error("生成图像失败");
    return;
  }
  try {
    await navigator.clipboard.write([
      new ClipboardItem({ "image/png": blob }),
    ]);
    toast.success("选区图像已复制到剪贴板");
  } catch (error) {
    toast.error(`复制失败：${error}`);
  }
}

/** 保存选区图像为 PNG 文件 */
async function saveRegionImage() {
  contextMenu.value = null;
  const cut = regionToCanvas();
  const object = projectStore.selectedObject;
  if (!cut || !object) {
    toast.error("当前选区无效");
    return;
  }
  const path = await save({
    title: "保存选区图像",
    defaultPath: `${object.name || "选区"}.png`,
    filters: [{ name: "PNG 图像", extensions: ["png"] }],
  });
  if (!path) return;
  const dataUrl = cut.toDataURL("image/png");
  try {
    await saveImagePng(path, dataUrl.slice(dataUrl.indexOf(",") + 1));
    toast.success(`已保存：${path}`);
  } catch (error) {
    toast.error(String(error));
  }
}

// ============ 特征组实时匹配预览 ============

/** 对象/点/绑定变化后防抖调度一次实时匹配 */
function scheduleLiveMatch() {
  liveMatchRequested = true;
  if (liveMatchTimer || liveMatchRunning) return;
  liveMatchTimer = setTimeout(() => {
    liveMatchTimer = null;
    void runLiveMatch();
  }, 150);
}

/** 对当前选中的对象运行匹配，结果用于画布点着色与右侧角标 */
async function runLiveMatch() {
  liveMatchRequested = false;
  const object = projectStore.selectedObject;
  if (
    !targetStore.bound ||
    !object ||
    !projectStore.selectedGroupId ||
    coordinateMismatch.value
  ) {
    liveMatch.value = null;
    void nextTick(draw);
    return;
  }
  liveMatchRunning = true;
  const sequence = ++liveMatchSequence;
  const targetId = targetStore.bound.targetId;
  const objectId = object.id;
  try {
    const target = projectStore.project.target;
    const resolved = resolveObjectForFrame(
      object,
      target.frameWidth,
      target.frameHeight,
      targetStore.frameWidth,
      targetStore.frameHeight,
    );
    const result = await runMatchAdvanced(
      targetId,
      resolved.region,
      resolved.groups,
      object.searchRadius,
      object.scaleSearchPercent,
    );
    if (
      sequence === liveMatchSequence &&
      targetStore.bound?.targetId === targetId &&
      projectStore.selectedObject?.id === objectId
    ) {
      liveMatch.value = result;
    }
  } catch {
    if (sequence === liveMatchSequence) liveMatch.value = null;
  } finally {
    liveMatchRunning = false;
    if (liveMatchRequested) scheduleLiveMatch();
  }
  void nextTick(draw);
}

// 切换到取色模式时把键盘光标放到当前对象区域中心
watch(mode, (value) => {
  if (value !== "pick") {
    pickCursor.value = null;
    return;
  }
  const fw = targetStore.frameWidth;
  const fh = targetStore.frameHeight;
  const object = projectStore.selectedObject;
  if (object) {
    pickCursor.value = {
      x: clamp(object.region.x + Math.floor(object.region.w / 2), 0, Math.max(0, fw - 1)),
      y: clamp(object.region.y + Math.floor(object.region.h / 2), 0, Math.max(0, fh - 1)),
    };
  } else if (fw && fh) {
    pickCursor.value = { x: Math.floor(fw / 2), y: Math.floor(fh / 2) };
  }
});

function pickColor(x: number, y: number) {
  const object = projectStore.selectedObject;
  const group = projectStore.selectedGroup;
  if (!object || !group) {
    toast.error("请先选择对象和特征组");
    return;
  }
  if (editingFrameMismatch.value) {
    toast.error("尺寸适配预览中只允许验证；请恢复基准尺寸后编辑区域和取色点");
    return;
  }
  const rgba = readPixel(x, y);
  if (!rgba) return;
  const dx = x - object.region.x;
  const dy = y - object.region.y;
  if (dx < 0 || dy < 0 || dx >= object.region.w || dy >= object.region.h) {
    toast.error("取色点必须位于当前对象区域内");
    return;
  }
  projectStore.addPoint(object.id, group.id, dx, dy, rgba);
  toast.success(`已添加特征点 (${dx}, ${dy}) ${rgbaToHex(rgba)}`);
}

async function onRefreshWindows() {
  await targetStore.refreshWindows();
}

/** 暂停/恢复预览：暂停后冻结当前帧，便于稳定标注 */
function togglePreviewPause() {
  if (targetStore.previewRunning) {
    targetStore.stopPreview();
  } else if (targetStore.bound) {
    targetStore.startPreview();
  }
}

/** 点击窗口项：最小化窗口立即经 winsitter 恢复到原位置，随后弹出绑定确认 */
async function onBind(itemIndex: number) {
  const item = targetStore.windows[itemIndex] ?? null;
  if (!item) return;
  if (item.state === WINDOW_STATE_MINIMIZED) {
    try {
      await restoreWindow(item.hwnd);
      toast.success("已恢复窗口到原位置（不抢前台）");
    } catch (error) {
      const message = String(error);
      // winsitter 0.5.5：仅当明确报权限不足（-1507，UIPI 拦截）时才动态弹 UAC 提权
      if (message.includes("权限不足")) {
        toast.warning(`恢复窗口被 Windows 拒绝：${message}，正在请求管理员权限…`);
        try {
          projectStore.flushRecovery();
          await relaunchElevated();
          toast.info("已以管理员身份重新启动本工具，请重新选择窗口");
        } catch (elevateError) {
          toast.error(String(elevateError));
        }
      } else {
        toast.error(`恢复窗口失败：${message}`);
      }
      return;
    }
  }
  pendingBind.value = item;
}

async function confirmBind() {
  const item = pendingBind.value;
  pendingBind.value = null;
  if (!item) return;
  try {
    await targetStore.bind(item);
    projectStore.setTargetHint(item.title, item.className, item.processId);
    toast.success(`已绑定：${item.title || item.className}`);
    screenOrigin.value = null;
    if (targetStore.bound) {
      const targetId = targetStore.bound.targetId;
      targetScreenOrigin(targetId)
        .then((origin) => {
          screenOrigin.value = origin;
        })
        .catch(() => {
          // 换算失败时仅显示相对坐标
        });
    }
    const warning = targetStore.bound?.restoreWarning;
    if (warning) toast.warning(warning, { duration: 8000 });
    scheduleLiveMatch();
  } catch {
    toast.error(targetStore.lastError || "绑定失败");
  }
}

async function onUnbind() {
  await targetStore.unbind();
  screenOrigin.value = null;
  liveMatch.value = null;
  toast.success("已解除绑定");
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <!-- 左侧：窗口列表 -->
    <div class="flex w-64 shrink-0 flex-col border-r">
      <div class="space-y-2 p-3">
        <div class="text-sm font-semibold">目标窗口</div>
        <div class="flex gap-1.5">
          <Input
            v-model="targetStore.windowFilter"
            class="h-8 text-xs"
            placeholder="按标题过滤"
            @keyup.enter="onRefreshWindows"
          />
          <Button
            size="icon"
            variant="outline"
            class="h-8 w-8 shrink-0"
            :disabled="targetStore.loading"
            @click="onRefreshWindows"
          >
            <Loader2 v-if="targetStore.loading" class="h-4 w-4 animate-spin" />
            <RefreshCw v-else class="h-4 w-4" />
          </Button>
        </div>
      </div>
      <div class="min-h-0 flex-1 overflow-auto px-2 pb-2">
        <div
          v-if="targetStore.lastError && !targetStore.bound"
          class="mb-2 rounded border border-destructive/50 p-2 text-xs text-destructive"
        >
          {{ targetStore.lastError }}
        </div>
        <button
          v-for="(item, index) in targetStore.windows"
          :key="item.hwnd"
          class="mb-1 w-full rounded-md border bg-background px-2.5 py-2 text-left transition-colors hover:border-primary cursor-pointer"
          @click="onBind(index)"
        >
          <div class="flex items-center gap-1.5">
            <span class="truncate text-sm">
              {{ item.title || `（无标题）${item.className}` }}
            </span>
            <Badge
              v-if="item.state === WINDOW_STATE_MINIMIZED"
              variant="secondary"
              class="shrink-0 text-[10px]"
            >
              已最小化
            </Badge>
          </div>
          <div class="truncate text-xs text-muted-foreground">
            {{ item.className }} · PID {{ item.processId }}
          </div>
        </button>
        <div
          v-if="!targetStore.windows.length && !targetStore.loading"
          class="px-2 py-6 text-center text-xs text-muted-foreground"
        >
          没有发现可见窗口
        </div>
      </div>
      <div v-if="targetStore.bound" class="border-t p-3">
        <div class="mb-1 text-xs text-muted-foreground">当前绑定</div>
        <div class="mb-2 truncate text-sm font-medium">
          {{ targetStore.bound.title }}
        </div>
        <Button size="sm" variant="outline" class="w-full" @click="onUnbind">
          <Unlink class="h-3.5 w-3.5" />
          解除绑定
        </Button>
      </div>
    </div>

    <!-- 中间：预览画布 -->
    <div class="flex min-w-0 flex-1 flex-col">
      <div class="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <Button
          size="sm"
          :variant="mode === 'region' ? 'default' : 'outline'"
          @click="mode = 'region'"
        >
          <MousePointer2 class="h-3.5 w-3.5" />
          框选区域
        </Button>
        <Button
          size="sm"
          :variant="mode === 'pick' ? 'default' : 'outline'"
          @click="mode = 'pick'"
        >
          <Pipette class="h-3.5 w-3.5" />
          取色加点
        </Button>
        <span
          v-if="targetStore.bound && !ctrlHeld"
          class="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
        >
          按住 Ctrl 才{{ mode === "region" ? "框选" : "取色" }}，直接拖拽可平移
        </span>
        <Separator orientation="vertical" class="h-5" />
        <div class="w-36">
          <Select
            :model-value="zoom"
            :options="zoomOptions"
            class="h-8 text-xs"
            @update:model-value="setZoom(String($event ?? 'fit'))"
          />
        </div>
        <span
          v-if="targetStore.bound"
          class="text-xs text-muted-foreground"
          title="按住 Ctrl 滚动滚轮也可缩放"
        >
          {{ Math.round(displayScale * 100) }}%
        </span>
        <Button
          v-if="targetStore.bound"
          size="icon"
          variant="outline"
          class="h-8 w-8"
          :title="targetStore.previewRunning ? '暂停预览（冻结当前帧）' : '恢复预览'"
          @click="togglePreviewPause"
        >
          <Pause v-if="targetStore.previewRunning" class="h-3.5 w-3.5" />
          <Play v-else class="h-3.5 w-3.5" />
        </Button>
        <span
          v-if="targetStore.bound && !targetStore.previewRunning"
          class="rounded bg-amber-400/15 px-1.5 py-0.5 text-[10px] text-amber-500"
        >
          已暂停，画面冻结
        </span>
        <span
          v-if="targetStore.bound && targetStore.lastError"
          class="max-w-72 truncate text-xs text-destructive"
          :title="targetStore.lastError"
        >
          {{ targetStore.lastError }}
        </span>
        <div class="flex-1" />
        <div class="text-xs text-muted-foreground">
          {{ targetStore.frameWidth || "-" }} ×
          {{ targetStore.frameHeight || "-" }} px
          <span v-if="coordinateMismatch" class="ml-2 text-destructive">
            固定坐标尺寸不兼容
          </span>
          <span v-else-if="editingFrameMismatch" class="ml-2 text-amber-500">
            适配预览（编辑已锁定）
          </span>
          <span v-if="hoverPixel" class="ml-2 tabular-nums">
            ({{ hoverPixel.x }}, {{ hoverPixel.y }})
          </span>
        </div>
      </div>

      <div class="flex min-h-0 flex-1">
        <!-- 左侧纵向标尺（y） -->
        <div class="flex shrink-0 select-none flex-col">
          <div
            class="flex items-center justify-center border-b border-r text-[8px] text-muted-foreground"
            :style="{ width: RULER_SIZE + 'px', height: RULER_SIZE + 'px' }"
            title="坐标均以图片左上角 (0,0) 为原点"
          >
            0,0
          </div>
          <canvas
            ref="rulerYRef"
            class="cursor-pointer border-r"
            :width="RULER_SIZE"
            :height="200"
            title="按住拖动创建/移动横参考线"
            @mousedown="onRulerMouseDown('y', $event)"
          />
        </div>
        <!-- 顶部横向标尺 + 视口 -->
        <div class="flex min-w-0 flex-1 flex-col">
          <canvas
            ref="rulerXRef"
            class="shrink-0 cursor-pointer select-none border-b"
            :width="400"
            :height="RULER_SIZE"
            title="按住拖动创建/移动竖参考线"
            @mousedown="onRulerMouseDown('x', $event)"
          />
          <div ref="wrapRef" class="relative min-h-0 flex-1">
            <div
              ref="viewportRef"
              class="checkerboard absolute inset-0 overflow-auto"
              @scroll="onViewportScroll"
              @mousedown.self="contextMenu = null"
            >
              <div
                v-if="!targetStore.bound"
                class="flex h-full items-center justify-center"
              >
                <div class="max-w-sm text-center text-sm text-muted-foreground">
                  <Crosshair class="mx-auto mb-3 h-10 w-10 opacity-40" />
                  <p>从左侧选择目标游戏窗口进行绑定。</p>
                  <p class="mt-1">
                    绑定后即可获得后台实时画面（不抢前台、不需要窗口可见），
                    在画布上拖拽框选对象区域，再用取色模式标记特征点。
                  </p>
                </div>
              </div>
              <canvas
                v-show="targetStore.bound"
                ref="canvasRef"
                class="m-3 block"
                :style="{ cursor: canvasCursor }"
                @mousemove="onMouseMove"
                @mousedown="onMouseDown"
                @dblclick="onCanvasDblClick"
                @contextmenu="onCanvasContextMenu"
                @mouseleave="hoverPixel = null"
              />
            </div>
            <!-- 参考线与坐标标签（相对图片左上角，括号内为屏幕位置） -->
            <template v-if="targetStore.bound">
              <div
                v-if="guideVPos != null"
                class="absolute bottom-0 top-0 z-10 w-[3px] cursor-col-resize bg-amber-400/70 hover:bg-amber-400"
                :style="{ left: guideVPos - 1 + 'px' }"
                title="拖动移动；双击输入精确 x 坐标"
                @mousedown="onGuideMouseDown('x', $event)"
                @dblclick="openGuideEditor('x')"
              />
              <div
                v-if="guideHPos != null"
                class="absolute left-0 right-0 z-10 h-[3px] cursor-row-resize bg-amber-400/70 hover:bg-amber-400"
                :style="{ top: guideHPos - 1 + 'px' }"
                title="拖动移动；双击输入精确 y 坐标"
                @mousedown="onGuideMouseDown('y', $event)"
                @dblclick="openGuideEditor('y')"
              />
              <div
                v-if="guideVPos != null"
                class="pointer-events-none absolute top-0 z-10 rounded-b bg-amber-400 px-1 py-0.5 text-[10px] font-medium text-black"
                :style="{ left: guideVPos + 5 + 'px' }"
              >
                {{ guideXLabel }}
              </div>
              <div
                v-if="guideHPos != null"
                class="pointer-events-none absolute left-0 z-10 rounded-r bg-amber-400 px-1 py-0.5 text-[10px] font-medium text-black"
                :style="{ top: guideHPos + 5 + 'px' }"
              >
                {{ guideYLabel }}
              </div>
              <input
                v-if="guideEditor"
                ref="guideInputRef"
                v-model="guideEditor.value"
                type="number"
                class="absolute z-20 h-6 w-24 rounded border bg-background px-1 text-xs"
                :style="guideEditorStyle"
                @keydown.enter.prevent="commitGuideEditor"
                @keydown.esc="guideEditor = null"
                @blur="commitGuideEditor"
              />
              <!-- 双击选区框边线后的坐标输入框 -->
              <input
                v-if="edgeEditor"
                ref="edgeInputRef"
                v-model="edgeEditor.value"
                type="number"
                class="absolute z-20 h-6 w-24 rounded border bg-background px-1 text-xs"
                :style="{
                  left: edgeEditor.left + 6 + 'px',
                  top: edgeEditor.top + 6 + 'px',
                }"
                :title="`设置选区${
                  edgeEditor.edge === 'top'
                    ? '上边 y'
                    : edgeEditor.edge === 'bottom'
                      ? '下边 y'
                      : edgeEditor.edge === 'left'
                        ? '左边 x'
                        : '右边 x'
                } 坐标（Enter 确认 / Esc 取消）`"
                @keydown="onEdgeEditorKeydown"
                @blur="commitEdgeEditor"
              />
            </template>
            <!-- 画布右键菜单 -->
            <div
              v-if="contextMenu"
              class="absolute z-30 w-44 rounded-md border bg-popover py-1 text-sm shadow-md"
              :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
            >
              <button
                class="w-full px-3 py-1.5 text-left hover:bg-accent cursor-pointer"
                @click="previewRegionImage"
              >
                预览选区图像…
              </button>
              <button
                class="w-full px-3 py-1.5 text-left hover:bg-accent cursor-pointer"
                @click="openCopyCoords"
              >
                复制坐标…
              </button>
              <button
                class="w-full px-3 py-1.5 text-left hover:bg-accent cursor-pointer"
                @click="copyRegionImage"
              >
                复制选区图像
              </button>
              <button
                class="w-full px-3 py-1.5 text-left hover:bg-accent cursor-pointer"
                @click="saveRegionImage"
              >
                保存选区图像为 PNG…
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 右侧：放大镜与当前选择 -->
    <div class="flex w-60 shrink-0 flex-col border-l">
      <div class="border-b p-3">
        <div class="mb-2 text-sm font-semibold">放大镜</div>
        <canvas
          ref="loupeRef"
          class="w-full rounded-md border bg-slate-800"
          width="160"
          height="160"
        />
        <div class="mt-2 flex h-8 items-center gap-2">
          <span
            class="inline-block h-5 w-5 rounded border"
            :style="{
              background: hoverPixel
                ? `rgba(${hoverPixel.rgba.join(',')})`
                : 'transparent',
            }"
          />
          <span class="text-xs tabular-nums text-muted-foreground">
            {{
              hoverPixel
                ? `R${hoverPixel.rgba[0]} G${hoverPixel.rgba[1]} B${hoverPixel.rgba[2]} A${hoverPixel.rgba[3]}`
                : "移动鼠标查看像素"
            }}
          </span>
        </div>
      </div>

      <div class="flex-1 space-y-3 overflow-auto p-3">
        <div>
          <div class="mb-1.5 text-xs text-muted-foreground">对象</div>
          <div class="flex gap-1.5">
            <Select
              :model-value="projectStore.selectedObjectId ?? undefined"
              :options="objectOptions"
              class="h-8 text-xs"
              placeholder="选择对象"
              @update:model-value="projectStore.selectObject($event)"
            />
          </div>
          <Button
            size="sm"
            variant="outline"
            class="mt-1.5 w-full"
            @click="
              projectStore.addObject(
                `对象${projectStore.project.objects.length + 1}`,
              )
            "
          >
            新建对象
          </Button>
        </div>

        <div>
          <div class="mb-1.5 text-xs text-muted-foreground">特征组（形态）</div>
          <Select
            :model-value="projectStore.selectedGroupId ?? undefined"
            :options="groupOptions"
            class="h-8 text-xs"
            placeholder="选择特征组"
            @update:model-value="projectStore.selectGroup($event)"
          />
          <Button
            v-if="projectStore.selectedObject"
            size="sm"
            variant="outline"
            class="mt-1.5 w-full"
            @click="
              projectStore.addGroup(
                projectStore.selectedObject!.id,
                `形态${projectStore.selectedObject!.groups.length + 1}`,
              )
            "
          >
            新建形态组
          </Button>
        </div>

        <div v-if="projectStore.selectedObject" class="rounded-md border p-2">
          <div class="mb-1 flex items-center justify-between">
            <span class="text-xs font-medium">
              {{ projectStore.selectedObject.name }}
            </span>
            <Badge variant="secondary" class="text-[10px]">
              {{ projectStore.selectedObject.region.w }}×{{
                projectStore.selectedObject.region.h
              }}
            </Badge>
          </div>
          <div class="text-xs text-muted-foreground">
            左上 ({{ regionCorners.topLeft }}) · 右下 ({{ regionCorners.bottomRight }})
          </div>
          <div class="mt-1 text-xs text-muted-foreground">
            当前组点数：{{ projectStore.selectedGroup?.points.length ?? 0 }}
          </div>
          <div v-if="liveGroupResult" class="mt-1.5 flex items-center gap-1.5">
            <Badge
              :variant="liveGroupResult.matched ? 'success' : 'destructive'"
              class="text-[10px]"
            >
              实时预览 {{ liveGroupResult.matched ? "命中" : "未命中" }}
            </Badge>
            <span class="text-[10px] tabular-nums text-muted-foreground">
              通过 {{ liveGroupResult.passedCount }}/{{ liveGroupResult.required }} 点
            </span>
          </div>
          <div v-else-if="liveMatch" class="mt-1.5">
            <Badge
              :variant="liveMatch.matched ? 'success' : 'secondary'"
              class="text-[10px]"
            >
              实时预览：对象{{ liveMatch.matched ? "命中" : "未命中" }}
            </Badge>
          </div>
        </div>

        <Button
          size="sm"
          variant="outline"
          class="w-full"
          @click="helpOpen = true"
        >
          <CircleHelp class="h-3.5 w-3.5" />
          帮助与操作说明
        </Button>
      </div>
    </div>

    <!-- 绑定前确认：提示保持窗口尺寸固定 -->
    <Dialog
      :open="!!pendingBind"
      title="绑定前确认"
      @update:open="(value) => { if (!value) pendingBind = null }"
    >
      <div v-if="pendingBind" class="space-y-3 text-sm">
        <p class="font-medium">
          即将绑定：{{ pendingBind.title || pendingBind.className }}
        </p>
        <p class="text-muted-foreground">
          绑定后请<b class="text-foreground">保持目标窗口大小固定不变</b>。
          之后记录的所有区域与特征点坐标都只对绑定时的窗口尺寸生效；
          中途改变窗口大小会导致已标注数据失效。
        </p>
        <ul class="list-disc space-y-1 pl-5 text-xs text-muted-foreground">
          <li v-if="pendingBind.state === WINDOW_STATE_MINIMIZED">
            该窗口此前已最小化，点击时已经 winsitter 接口恢复到原位置（不抢前台、保持原 Z 序）。
          </li>
          <li>捕获在后台进行，无需窗口处于前台或可见。</li>
          <li>若目标程序以管理员运行导致操作失败，会自动弹出 UAC 提权并重启本工具。</li>
        </ul>
        <div class="flex justify-end gap-2 pt-1">
          <Button variant="outline" size="sm" @click="pendingBind = null">
            取消
          </Button>
          <Button size="sm" @click="confirmBind">确认绑定</Button>
        </div>
      </div>
    </Dialog>

    <!-- 帮助与操作说明 -->
    <Dialog
      :open="helpOpen"
      title="帮助与操作说明"
      @update:open="(value) => (helpOpen = !!value)"
    >
      <div class="max-h-[70vh] space-y-3 overflow-auto text-xs leading-relaxed text-muted-foreground">
        <section>
          <p class="mb-1 font-medium text-foreground">画布与缩放</p>
          <p>左键拖拽（不按 Ctrl）或中键拖拽可平移；Ctrl + 滚轮或工具栏下拉缩放，支持 10%～1600%，像素级查看。工具栏可暂停预览冻结当前帧，便于对动态画面稳定标注，再点恢复。</p>
        </section>
        <section>
          <p class="mb-1 font-medium text-foreground">框选区域模式</p>
          <ul class="list-disc space-y-0.5 pl-4">
            <li>按住 Ctrl 在空白处拖拽画新区域；区域内拖拽整体移动；拖边框/角点调整大小。</li>
            <li>方向键微调位置（Shift = 10px）。</li>
            <li>双击选区框的横线可输入 y 坐标（上/下边），双击纵线输入 x 坐标（左/右边）。</li>
            <li>右键画布：复制选区图像到剪贴板 / 保存选区为 PNG。</li>
          </ul>
        </section>
        <section>
          <p class="mb-1 font-medium text-foreground">取色加点模式</p>
          <p>
            按住 Ctrl 点击像素添加特征点；方向键移动光标（Shift = 10px），Enter / 空格确认加点。
            选中特征组后会自动实时匹配，画布上的点按通过（绿）/失败（红）着色。
          </p>
        </section>
        <section>
          <p class="mb-1 font-medium text-foreground">标尺与参考线</p>
          <p>
            在顶部/左侧标尺上拖动创建参考线，拖动线体移动；双击参考线输入精确坐标。
            坐标默认相对图片左上角 (0,0)，括号内为屏幕位置。
          </p>
        </section>
        <section>
          <p class="mb-1 font-medium text-foreground">改名</p>
          <p>在对象与特征页可修改对象名与形态组名（双击列表项/标签页也可快速改名）。</p>
        </section>
        <div class="flex justify-end pt-1">
          <Button size="sm" variant="outline" @click="helpOpen = false">
            关闭
          </Button>
        </div>
      </div>
    </Dialog>

    <!-- 选区图像预览 -->
    <Dialog
      :open="regionPreviewOpen"
      title="选区图像预览"
      @update:open="(value) => (regionPreviewOpen = !!value)"
    >
      <div class="space-y-3">
        <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <Badge variant="secondary">
            {{ projectStore.selectedObject?.name ?? "未选择对象" }}
          </Badge>
          <span>
            {{ projectStore.selectedObject?.region.w ?? 0 }}×{{
              projectStore.selectedObject?.region.h ?? 0
            }}
            像素 · 左上 ({{ regionCorners.topLeft }}) · 右下 ({{
              regionCorners.bottomRight
            }})
          </span>
        </div>
        <div class="checkerboard max-h-[65vh] overflow-auto rounded-md border p-2">
          <img
            v-if="regionPreviewUrl"
            :src="regionPreviewUrl"
            alt="选区图像预览"
            class="mx-auto block max-w-full"
            style="image-rendering: pixelated"
          />
        </div>
        <p class="text-xs text-muted-foreground">
          截图取自当前预览帧（1:1 原始像素，与复制/保存的选区图像完全一致）。
        </p>
        <div class="flex justify-end">
          <Button size="sm" variant="outline" @click="regionPreviewOpen = false">
            关闭
          </Button>
        </div>
      </div>
    </Dialog>

    <!-- 复制坐标：多规格对照 -->
    <Dialog
      :open="coordsOpen"
      title="复制选区坐标"
      @update:open="(value) => (coordsOpen = !!value)"
    >
      <div class="space-y-3">
        <p class="text-xs leading-relaxed text-muted-foreground">
          坐标均基于目标窗口客户区（已去窗口边框）。矩形有两种常见口径：
          <span class="text-foreground">左闭右开</span>（right/bottom 不含边界像素，宽
          = right - left，winsitter、PIL、NumPy、Win32 RECT 都用这种）；
          <span class="text-foreground">双闭</span>（右下角是包含的末像素，宽
          = 右下 - 左上 + 1，本工具界面显示用这种）。两种口径指同一块像素，只是写法不同，按脚本习惯选用即可。
        </p>
        <div class="max-h-[60vh] space-y-2 overflow-auto">
          <div
            v-for="item in coordFormats"
            :key="item.name"
            class="rounded-md border p-2.5"
          >
            <div class="mb-1 flex items-center justify-between gap-2">
              <span class="text-xs font-medium">{{ item.name }}</span>
              <Button
                size="sm"
                variant="outline"
                class="h-6 px-2 text-xs"
                @click="copyCoordValue(item.value)"
              >
                <Copy class="h-3 w-3" />
                复制
              </Button>
            </div>
            <code
              class="block break-all rounded bg-muted px-2 py-1.5 text-xs"
            >{{ item.value }}</code>
            <p class="mt-1 text-xs leading-relaxed text-muted-foreground">
              {{ item.desc }}
            </p>
          </div>
        </div>
        <div class="flex justify-end">
          <Button size="sm" variant="outline" @click="coordsOpen = false">
            关闭
          </Button>
        </div>
      </div>
    </Dialog>
  </div>
</template>

<style scoped>
/* 棋盘格背景便于观察透明区域 */
.checkerboard {
  background-image:
    linear-gradient(45deg, var(--muted) 25%, transparent 25%),
    linear-gradient(-45deg, var(--muted) 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, var(--muted) 75%),
    linear-gradient(-45deg, transparent 75%, var(--muted) 75%);
  background-size: 16px 16px;
  background-position:
    0 0,
    0 8px,
    8px -8px,
    -8px 0;
}
</style>
