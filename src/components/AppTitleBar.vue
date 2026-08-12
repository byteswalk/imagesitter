<script setup lang="ts">
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { Crosshair, Minus, Square, X } from "lucide-vue-next";
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useProjectStore } from "@/stores/project";
import Tooltip from "@/components/ui/tooltip/Tooltip.vue";

// 纯浏览器预览（无 Tauri 宿主）时降级为静态标题栏，避免挂载失败
const isDesktop = "__TAURI_INTERNALS__" in window;
const appWindow: Window | null = isDesktop ? getCurrentWindow() : null;
const projectStore = useProjectStore();

const maximized = ref(false);
let removeResizeListener: (() => void) | null = null;

const projectLabel = computed(() => {
  const name = projectStore.filePath
    ? projectStore.filePath.split(/[\\/]/).pop()
    : "未保存项目";
  return projectStore.dirty ? `${name} ·` : name;
});

async function syncMaximized() {
  if (!appWindow) return;
  maximized.value = await appWindow.isMaximized();
}

onMounted(async () => {
  if (!appWindow) return;
  void syncMaximized();
  removeResizeListener = await appWindow.onResized(() => void syncMaximized());
});

onUnmounted(() => removeResizeListener?.());

async function toggleMaximize() {
  if (!appWindow) return;
  await appWindow.toggleMaximize();
  await syncMaximized();
}

function minimize() {
  void appWindow?.minimize();
}

function close() {
  void appWindow?.close();
}
</script>

<template>
  <header
    data-tauri-drag-region
    class="flex h-9 shrink-0 select-none items-center border-b bg-card pl-3"
  >
    <div data-tauri-drag-region class="flex items-center gap-2">
      <Crosshair class="h-4 w-4 text-primary" />
      <span class="text-sm font-semibold">ImageSitter</span>
      <span class="ml-2 text-xs text-muted-foreground">{{ projectLabel }}</span>
    </div>

    <div class="flex-1" data-tauri-drag-region />

    <div class="flex h-full items-stretch">
      <Tooltip text="最小化">
        <button
          class="flex w-11 items-center justify-center transition-colors hover:bg-accent cursor-pointer"
          aria-label="最小化"
          @click="minimize()"
        >
          <Minus class="h-4 w-4" />
        </button>
      </Tooltip>
      <Tooltip :text="maximized ? '向下还原' : '最大化'">
        <button
          class="flex w-11 items-center justify-center transition-colors hover:bg-accent cursor-pointer"
          :aria-label="maximized ? '向下还原' : '最大化'"
          @click="toggleMaximize()"
        >
          <Square v-if="maximized" class="h-3 w-3" />
          <Square v-else class="h-3.5 w-3.5" />
        </button>
      </Tooltip>
      <Tooltip text="关闭">
        <button
          class="flex w-11 items-center justify-center transition-colors hover:bg-destructive hover:text-destructive-foreground cursor-pointer"
          aria-label="关闭"
          @click="close()"
        >
          <X class="h-4 w-4" />
        </button>
      </Tooltip>
    </div>
  </header>
</template>
