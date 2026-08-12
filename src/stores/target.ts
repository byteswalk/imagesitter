/**
 * 目标窗口 store：窗口列表、绑定会话与实时预览帧。
 * 预览帧以 ImageBitmap 保存，供捕获页 canvas 绘制与取色。
 */
import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";
import { toast } from "vue-sonner";
import type { BoundTarget, WindowItem } from "@/lib/types";
import {
  bindTarget,
  captureFramePng,
  listWindows,
  unbindTarget,
} from "@/lib/ipc";

export const useTargetStore = defineStore("target", () => {
  const windows = ref<WindowItem[]>([]);
  const windowFilter = ref("");
  const bound = ref<BoundTarget | null>(null);
  const loading = ref(false);
  const lastError = ref("");

  /** 当前帧位图；shallowRef 避免深度响应开销 */
  const frameBitmap = shallowRef<ImageBitmap | null>(null);
  const frameWidth = ref(0);
  const frameHeight = ref(0);
  const frameUpdatedAt = ref(0);
  const currentDpi = ref(0);
  const previewRunning = ref(false);
  /** 绑定后首帧的尺寸基准；所有区域/特征点坐标均相对该尺寸 */
  const baseSize = ref<{ w: number; h: number } | null>(null);
  /** 帧尺寸与基准不一致（已提示过，避免刷屏） */
  const sizeMismatch = ref(false);

  let loopTimer: ReturnType<typeof setTimeout> | null = null;
  let fetching = false;
  let disposed = false;
  let bindingGeneration = 0;

  async function refreshWindows() {
    loading.value = true;
    lastError.value = "";
    try {
      windows.value = await listWindows(windowFilter.value || undefined);
    } catch (error) {
      lastError.value = String(error);
    } finally {
      loading.value = false;
    }
  }

  async function bind(item: WindowItem) {
    lastError.value = "";
    const previous = bound.value;
    const resumePrevious = previewRunning.value;
    stopPreview();
    const generation = ++bindingGeneration;
    try {
      const result = await bindTarget(item.hwnd, item.title, item.state);
      if (generation !== bindingGeneration) {
        await unbindTarget(result.targetId).catch(() => undefined);
        return;
      }
      bound.value = result;
      currentDpi.value = item.dpi;
      if (previous) {
        await unbindTarget(previous.targetId).catch(() => undefined);
      }
      frameBitmap.value?.close();
      frameBitmap.value = null;
      frameWidth.value = 0;
      frameHeight.value = 0;
      frameUpdatedAt.value = 0;
      baseSize.value = null;
      sizeMismatch.value = false;
      startPreview();
    } catch (error) {
      lastError.value = String(error);
      bound.value = previous;
      if (previous && resumePrevious) startPreview();
      throw error;
    }
  }

  async function unbind() {
    bindingGeneration++;
    stopPreview();
    if (bound.value) {
      try {
        await unbindTarget(bound.value.targetId);
      } catch {
        // 窗口可能已关闭，忽略释放失败
      }
    }
    bound.value = null;
    currentDpi.value = 0;
    baseSize.value = null;
    sizeMismatch.value = false;
    frameBitmap.value?.close();
    frameBitmap.value = null;
    frameWidth.value = 0;
    frameHeight.value = 0;
  }

  /** 抓取单帧并更新位图；供预览循环和手动刷新复用。 */
  async function fetchFrame(): Promise<boolean> {
    if (!bound.value || fetching || disposed) return false;
    const targetId = bound.value.targetId;
    const generation = bindingGeneration;
    fetching = true;
    try {
      const payload = await captureFramePng(targetId);
      const blob = await (await fetch(payload.pngDataUrl)).blob();
      const bitmap = await createImageBitmap(blob);
      if (
        generation !== bindingGeneration ||
        bound.value?.targetId !== targetId ||
        disposed
      ) {
        bitmap.close();
        return false;
      }
      frameBitmap.value?.close();
      frameBitmap.value = bitmap;
      frameWidth.value = payload.width;
      frameHeight.value = payload.height;
      frameUpdatedAt.value = Date.now();
      lastError.value = "";
      if (!baseSize.value) {
        baseSize.value = { w: payload.width, h: payload.height };
      } else if (
        !sizeMismatch.value &&
        (baseSize.value.w !== payload.width ||
          baseSize.value.h !== payload.height)
      ) {
        sizeMismatch.value = true;
        toast.error(
          `窗口尺寸已从 ${baseSize.value.w}×${baseSize.value.h} 变为 ${payload.width}×${payload.height}，已记录的区域与特征点坐标可能失效，请保持窗口尺寸固定`,
        );
      }
      return true;
    } catch (error) {
      lastError.value = String(error);
      return false;
    } finally {
      fetching = false;
    }
  }

  /** 启动约 10fps 的预览循环；上一帧完成后再排下一帧。 */
  function startPreview(intervalMs = 100) {
    if (!bound.value || previewRunning.value) return;
    previewRunning.value = true;
    disposed = false;
    const tick = async () => {
      if (!previewRunning.value || disposed) return;
      const ok = await fetchFrame();
      if (!previewRunning.value || disposed) return;
      // 取帧失败（如窗口最小化）时放慢重试节奏
      loopTimer = setTimeout(tick, ok ? intervalMs : 1000);
    };
    void tick();
  }

  function stopPreview() {
    previewRunning.value = false;
    if (loopTimer) {
      clearTimeout(loopTimer);
      loopTimer = null;
    }
  }

  function dispose() {
    disposed = true;
    stopPreview();
  }

  return {
    windows,
    windowFilter,
    bound,
    loading,
    lastError,
    frameBitmap,
    frameWidth,
    frameHeight,
    frameUpdatedAt,
    currentDpi,
    previewRunning,
    baseSize,
    sizeMismatch,
    refreshWindows,
    bind,
    unbind,
    fetchFrame,
    startPreview,
    stopPreview,
    dispose,
  };
});
