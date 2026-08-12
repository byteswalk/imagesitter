<script setup lang="ts">
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import {
  Crosshair,
  FileDown,
  FolderOpen,
  GitMerge,
  History,
  Monitor,
  Redo2,
  Save,
  Settings,
  Shapes,
  Undo2,
} from "lucide-vue-next";
import { computed } from "vue";
import { RouterLink } from "vue-router";
import { toast } from "vue-sonner";
import Badge from "@/components/ui/badge/Badge.vue";
import Separator from "@/components/ui/separator/Separator.vue";
import Tooltip from "@/components/ui/tooltip/Tooltip.vue";
import { cn } from "@/lib/utils";
import { openProjectFile } from "@/lib/ipc";
import { parseProject, useProjectStore } from "@/stores/project";
import { useTargetStore } from "@/stores/target";

const projectStore = useProjectStore();
const targetStore = useTargetStore();

const navItems = [
  { to: "/", icon: Monitor, label: "捕获", exact: true },
  { to: "/objects", icon: Shapes, label: "对象与特征" },
  { to: "/calibrate", icon: Crosshair, label: "校准" },
  { to: "/replay", icon: History, label: "回放测试" },
  { to: "/export", icon: FileDown, label: "导出" },
  { to: "/settings", icon: Settings, label: "设置" },
];

const targetLabel = computed(
  () => targetStore.bound?.title || "未绑定目标窗口",
);

async function openProject() {
  if (projectStore.dirty) {
    projectStore.flushRecovery();
    const approved = await confirm(
      "当前项目有未保存修改，打开其他项目会替换当前工作区并清除它的自动恢复副本，是否继续？",
      { title: "打开其他项目", kind: "warning" },
    );
    if (!approved) return;
  }
  const path = await folderOpenDialog();
  if (!path) return;
  try {
    await projectStore.loadFrom(path);
    targetStore.windowFilter = projectStore.project.target.windowTitle;
    toast.success("项目已打开");
  } catch (error) {
    toast.error(String(error));
  }
}

async function folderOpenDialog(): Promise<string | null> {
  const result = await open({
    title: "打开 ImageSitter 项目",
    filters: [{ name: "ImageSitter 项目", extensions: ["json", "imst"] }],
    multiple: false,
  });
  return typeof result === "string" ? result : null;
}

async function saveProject() {
  let path = projectStore.filePath;
  if (!path) {
    path =
      (await save({
        title: "保存 ImageSitter 项目",
        defaultPath: "imagesitter-project.json",
        filters: [{ name: "ImageSitter 项目", extensions: ["json"] }],
      })) ?? null;
  }
  if (!path) return;
  try {
    await projectStore.saveTo(path);
    toast.success("项目已保存");
  } catch (error) {
    toast.error(String(error));
  }
}

async function mergeProject() {
  const path = await folderOpenDialog();
  if (!path) return;
  try {
    const incoming = parseProject(await openProjectFile(path));
    const groups = incoming.objects.reduce((sum, object) => sum + object.groups.length, 0);
    const points = incoming.objects.reduce(
      (sum, object) => sum + object.groups.reduce((count, group) => count + group.points.length, 0),
      0,
    );
    const currentSize = `${projectStore.project.target.frameWidth}×${projectStore.project.target.frameHeight}`;
    const incomingSize = `${incoming.target.frameWidth}×${incoming.target.frameHeight}`;
    const approved = await confirm(
      `项目对比\n\n当前：${projectStore.project.objects.length} 对象，基准 ${currentSize}\n待合并：${incoming.objects.length} 对象 / ${groups} 状态 / ${points} 特征点，基准 ${incomingSize}\n\n将合并规则并重新分配 ID；${incoming.replayCases.length} 个回放样本不会合并，避免外部路径失效。继续吗？`,
      { title: "对比并合并项目", kind: "warning" },
    );
    if (!approved) return;
    const merged = projectStore.mergeProject(incoming);
    toast.success(`已合并 ${merged.objects} 个对象、${merged.groups} 个状态、${merged.points} 个特征点`);
  } catch (error) {
    toast.error(String(error));
  }
}
</script>

<template>
  <aside
    class="flex w-48 shrink-0 flex-col border-r bg-card/50"
    aria-label="主导航"
  >
    <nav class="flex flex-1 flex-col gap-1 p-2">
      <RouterLink
        v-for="item in navItems"
        :key="item.to"
        :to="item.to"
        custom
      >
        <template #default="{ navigate, isActive }">
          <button
            :class="
              cn(
                'flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-sm transition-colors cursor-pointer',
                isActive || (item.exact && $route.path === item.to)
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
              )
            "
            @click="navigate"
          >
            <component :is="item.icon" class="h-4 w-4 shrink-0" />
            {{ item.label }}
          </button>
        </template>
      </RouterLink>
    </nav>

    <Separator />

    <div class="space-y-2 p-3">
      <div class="text-xs text-muted-foreground">目标窗口</div>
      <Tooltip :text="targetLabel">
        <Badge
          :variant="targetStore.bound ? 'success' : 'secondary'"
          class="max-w-full truncate"
        >
          {{ targetLabel }}
        </Badge>
      </Tooltip>
      <div class="flex gap-1.5">
        <Tooltip text="撤销 (Ctrl+Z)">
          <button
            class="flex h-8 flex-1 items-center justify-center rounded-md border bg-background transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40 cursor-pointer"
            aria-label="撤销"
            :disabled="!projectStore.canUndo"
            @click="projectStore.undo"
          >
            <Undo2 class="h-4 w-4" />
          </button>
        </Tooltip>
        <Tooltip text="重做 (Ctrl+Y)">
          <button
            class="flex h-8 flex-1 items-center justify-center rounded-md border bg-background transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40 cursor-pointer"
            aria-label="重做"
            :disabled="!projectStore.canRedo"
            @click="projectStore.redo"
          >
            <Redo2 class="h-4 w-4" />
          </button>
        </Tooltip>
      </div>
      <div class="flex gap-1.5">
        <Tooltip text="打开项目">
          <button
            class="flex h-8 flex-1 items-center justify-center rounded-md border bg-background transition-colors hover:bg-accent cursor-pointer"
            aria-label="打开项目"
            @click="openProject"
          >
            <FolderOpen class="h-4 w-4" />
          </button>
        </Tooltip>
        <Tooltip text="保存项目">
          <button
            class="flex h-8 flex-1 items-center justify-center rounded-md border bg-background transition-colors hover:bg-accent cursor-pointer"
            aria-label="保存项目"
            @click="saveProject"
          >
            <Save class="h-4 w-4" />
          </button>
        </Tooltip>
      </div>
      <Tooltip text="对比并合并另一项目的规则">
        <button
          class="flex h-8 w-full items-center justify-center gap-1.5 rounded-md border bg-background text-xs transition-colors hover:bg-accent cursor-pointer"
          aria-label="合并项目"
          @click="mergeProject"
        >
          <GitMerge class="h-3.5 w-3.5" />
          对比 / 合并项目
        </button>
      </Tooltip>
    </div>
  </aside>
</template>
