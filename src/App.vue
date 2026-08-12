<script setup lang="ts">
import { Toaster } from "vue-sonner";
import { confirm } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMounted, onUnmounted } from "vue";
import { toast } from "vue-sonner";
import AppTitleBar from "@/components/AppTitleBar.vue";
import AppSidebar from "@/components/AppSidebar.vue";
import { useSettingsStore } from "@/stores/settings";
import { useProjectStore } from "@/stores/project";

// 初始化主题 store 以应用保存的主题偏好
useSettingsStore();
const projectStore = useProjectStore();
const appWindow = "__TAURI_INTERNALS__" in window ? getCurrentWindow() : null;
let allowClose = false;
let removeCloseListener: (() => void) | null = null;

function onProjectShortcut(event: KeyboardEvent) {
  if (!event.ctrlKey || event.altKey) return;
  const target = event.target as HTMLElement | null;
  if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
  if (event.key.toLowerCase() === "z") {
    event.preventDefault();
    if (event.shiftKey) projectStore.redo();
    else projectStore.undo();
  } else if (event.key.toLowerCase() === "y") {
    event.preventDefault();
    projectStore.redo();
  }
}

onMounted(async () => {
  window.addEventListener("keydown", onProjectShortcut);
  const restoredAt = projectStore.restoreRecovery();
  if (restoredAt) {
    toast.warning(
      `已恢复 ${new Date(restoredAt).toLocaleString()} 的未保存项目${projectStore.replayRecoveryOmitted ? "；因容量限制，回放 PNG 未包含在恢复副本中" : ""}，请确认后保存`,
      { duration: 8000 },
    );
  }
  if (!appWindow) return;
  removeCloseListener = await appWindow.onCloseRequested(async (event) => {
    if (allowClose || !projectStore.dirty) return;
    event.preventDefault();
    projectStore.flushRecovery();
    const approved = await confirm(
      projectStore.replayRecoveryOmitted
        ? "当前项目有未保存修改，且回放 PNG 因容量限制未写入自动恢复副本；关闭会丢失这些未保存样本。确定关闭吗？"
        : "当前项目有未保存修改。关闭后仍可从自动恢复副本继续，但建议先保存。确定关闭吗？",
      { title: "未保存的项目", kind: "warning" },
    );
    if (approved) {
      allowClose = true;
      await appWindow.close();
    }
  });
});

onUnmounted(() => {
  window.removeEventListener("keydown", onProjectShortcut);
  removeCloseListener?.();
});
</script>

<template>
  <div class="flex h-full flex-col bg-background text-foreground">
    <AppTitleBar />
    <div class="flex min-h-0 flex-1">
      <AppSidebar />
      <main class="min-w-0 flex-1 overflow-auto">
        <RouterView />
      </main>
    </div>
    <Toaster position="bottom-right" :duration="3500" rich-colors />
  </div>
</template>
