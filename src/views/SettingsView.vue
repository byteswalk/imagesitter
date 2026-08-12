<script setup lang="ts">
import { confirm, save } from "@tauri-apps/plugin-dialog";
import { Download, FolderCheck, History, Monitor, Moon, RefreshCw, Sun, Trash2 } from "lucide-vue-next";
import { onMounted, ref, watch } from "vue";
import { toast } from "vue-sonner";
import Button from "@/components/ui/button/Button.vue";
import Card from "@/components/ui/card/Card.vue";
import CardContent from "@/components/ui/card/CardContent.vue";
import CardHeader from "@/components/ui/card/CardHeader.vue";
import CardTitle from "@/components/ui/card/CardTitle.vue";
import { cn } from "@/lib/utils";
import {
  exportDiagnostics,
  auditProjectSamples,
  cleanupOrphanSamples,
  listProjectHistory,
  runtimeDiagnostics,
  type ProjectHistoryEntry,
  type RuntimeDiagnostics,
  type SampleAudit,
} from "@/lib/ipc";
import { useProjectStore } from "@/stores/project";
import { useSettingsStore, type ThemeMode } from "@/stores/settings";

const settings = useSettingsStore();
const projectStore = useProjectStore();
const historyEntries = ref<ProjectHistoryEntry[]>([]);
const diagnostics = ref<RuntimeDiagnostics | null>(null);
const loadingHistory = ref(false);
const sampleAudit = ref<SampleAudit | null>(null);
const auditingSamples = ref(false);

const themeOptions: { value: ThemeMode; label: string; icon: unknown }[] = [
  { value: "light", label: "浅色", icon: Sun },
  { value: "dark", label: "深色", icon: Moon },
  { value: "system", label: "跟随系统", icon: Monitor },
];

function formatBytes(size: number) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}

async function refreshHistory() {
  if (!projectStore.filePath) {
    historyEntries.value = [];
    return;
  }
  loadingHistory.value = true;
  try {
    historyEntries.value = await listProjectHistory(projectStore.filePath);
  } catch (error) {
    toast.error(String(error));
  } finally {
    loadingHistory.value = false;
  }
}

async function restoreHistory(entry: ProjectHistoryEntry) {
  const approved = await confirm(
    `恢复 ${new Date(entry.savedAt).toLocaleString()} 的项目快照？\n\n当前内容不会立即覆盖磁盘，恢复后仍需手动保存。`,
    { title: "恢复项目历史", kind: "warning" },
  );
  if (!approved) return;
  try {
    await projectStore.restoreHistory(entry.fileName);
    toast.success("历史快照已恢复为未保存状态");
  } catch (error) {
    toast.error(String(error));
  }
}

async function exportDiagnosticFile() {
  const path = await save({
    title: "导出 ImageSitter 诊断信息",
    defaultPath: `imagesitter-diagnostics-${Date.now()}.json`,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!path) return;
  const objects = projectStore.project.objects.length;
  const groups = projectStore.project.objects.reduce((sum, object) => sum + object.groups.length, 0);
  const points = projectStore.project.objects.reduce(
    (sum, object) => sum + object.groups.reduce((count, group) => count + group.points.length, 0),
    0,
  );
  try {
    await exportDiagnostics(path, {
      projectVersion: projectStore.project.version,
      objects,
      groups,
      points,
      replayCases: projectStore.project.replayCases.length,
      dirty: projectStore.dirty,
    });
    toast.success("诊断信息已导出；不包含截图、路径、窗口标题或凭据");
  } catch (error) {
    toast.error(String(error));
  }
}

async function auditSamples() {
  if (!projectStore.filePath) return;
  if (projectStore.dirty) {
    toast.error("请先保存项目，再以磁盘版本为准检查和清理");
    return;
  }
  auditingSamples.value = true;
  try {
    sampleAudit.value = await auditProjectSamples(
      projectStore.filePath,
      projectStore.project.replayCases
        .filter((item) => item.storage === "external")
        .map((item) => ({ relativePath: item.relativePath, sha256: item.sha256 })),
    );
    const issues = sampleAudit.value.missing.length + sampleAudit.value.modified.length;
    if (issues) toast.error(`发现 ${issues} 个缺失或被修改的外置样本`);
    else toast.success("外置样本完整性检查通过");
  } catch (error) {
    toast.error(String(error));
  } finally {
    auditingSamples.value = false;
  }
}

async function cleanOrphans() {
  if (!projectStore.filePath || !sampleAudit.value?.orphaned.length) return;
  const approved = await confirm(
    `永久删除 ${sampleAudit.value.orphaned.length} 个未被当前项目引用的受管 PNG？此操作不能撤销。`,
    { title: "清理无引用样本", kind: "warning" },
  );
  if (!approved) return;
  try {
    const removed = await cleanupOrphanSamples(projectStore.filePath, sampleAudit.value.orphaned);
    toast.success(`已删除 ${removed} 个无引用样本`);
    await auditSamples();
  } catch (error) {
    toast.error(String(error));
  }
}

onMounted(async () => {
  await refreshHistory();
  try {
    diagnostics.value = await runtimeDiagnostics();
  } catch {
    // 浏览器预览模式没有 Tauri IPC。
  }
});
watch(() => projectStore.filePath, () => void refreshHistory());
</script>

<template>
  <div class="mx-auto max-w-3xl space-y-4 p-6">
    <h1 class="text-lg font-semibold">设置与维护</h1>

    <Card>
      <CardHeader><CardTitle class="text-sm">主题</CardTitle></CardHeader>
      <CardContent>
        <div class="flex gap-2">
          <button
            v-for="option in themeOptions"
            :key="option.value"
            :class="cn(
              'flex flex-1 flex-col items-center gap-2 rounded-lg border p-4 text-sm transition-colors cursor-pointer',
              settings.theme === option.value
                ? 'border-primary bg-primary/10 text-primary'
                : 'hover:bg-accent',
            )"
            @click="settings.setTheme(option.value)"
          >
            <component :is="option.icon" class="h-5 w-5" />
            {{ option.label }}
          </button>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader class="flex-row items-center justify-between space-y-0">
        <CardTitle class="flex items-center gap-2 text-sm">
          <History class="h-4 w-4" />项目历史
        </CardTitle>
        <Button size="sm" variant="outline" :disabled="loadingHistory || !projectStore.filePath" @click="refreshHistory">
          <RefreshCw class="h-3.5 w-3.5" />刷新
        </Button>
      </CardHeader>
      <CardContent>
        <p v-if="!projectStore.filePath" class="text-sm text-muted-foreground">
          保存项目后，每次覆盖保存前会自动保留一个快照，最多 30 个。
        </p>
        <p v-else-if="!historyEntries.length" class="text-sm text-muted-foreground">
          暂无历史。下次对已保存项目写入新修改时会自动创建。
        </p>
        <div v-else class="max-h-56 space-y-1 overflow-auto">
          <div
            v-for="entry in historyEntries"
            :key="entry.fileName"
            class="flex items-center gap-3 rounded-md border px-3 py-2 text-sm"
          >
            <span class="flex-1 tabular-nums">{{ new Date(entry.savedAt).toLocaleString() }}</span>
            <span class="text-xs text-muted-foreground">{{ formatBytes(entry.size) }}</span>
            <Button size="sm" variant="outline" @click="restoreHistory(entry)">恢复</Button>
          </div>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader class="flex-row items-center justify-between space-y-0">
        <CardTitle class="flex items-center gap-2 text-sm">
          <FolderCheck class="h-4 w-4" />外置样本维护
        </CardTitle>
        <Button size="sm" variant="outline" :disabled="auditingSamples || !projectStore.filePath || projectStore.dirty" @click="auditSamples">
          <RefreshCw class="h-3.5 w-3.5" />{{ auditingSamples ? "检查中…" : "检查完整性" }}
        </Button>
      </CardHeader>
      <CardContent class="space-y-3 text-sm">
        <p v-if="!projectStore.filePath" class="text-muted-foreground">先保存项目，才能检查其受管 `.samples` 目录。</p>
        <p v-else-if="!sampleAudit" class="text-muted-foreground">检查 SHA-256、缺失文件和不再被当前项目引用的残留 PNG。</p>
        <template v-else>
          <div class="grid grid-cols-3 gap-2 text-center text-xs">
            <div class="rounded-md bg-muted p-2"><div class="text-lg font-semibold">{{ sampleAudit.missing.length }}</div>缺失</div>
            <div class="rounded-md bg-muted p-2"><div class="text-lg font-semibold">{{ sampleAudit.modified.length }}</div>被修改</div>
            <div class="rounded-md bg-muted p-2"><div class="text-lg font-semibold">{{ sampleAudit.orphaned.length }}</div>无引用</div>
          </div>
          <Button v-if="sampleAudit.orphaned.length" size="sm" variant="destructive" @click="cleanOrphans">
            <Trash2 class="h-3.5 w-3.5" />清理无引用样本
          </Button>
        </template>
      </CardContent>
    </Card>

    <Card>
      <CardHeader><CardTitle class="text-sm">诊断与发布状态</CardTitle></CardHeader>
      <CardContent class="space-y-3 text-sm text-muted-foreground">
        <div v-if="diagnostics" class="grid grid-cols-2 gap-2 rounded-md bg-muted/40 p-3 text-xs">
          <span>版本</span><span>ImageSitter v{{ diagnostics.appVersion }}</span>
          <span>系统</span><span>{{ diagnostics.operatingSystem }} / {{ diagnostics.architecture }}</span>
          <span>winsitter.dll</span><span>{{ diagnostics.winsitterDllPresent ? "已就绪" : "未在程序目录发现" }}</span>
        </div>
        <p>诊断文件仅包含版本、系统架构、DLL 是否存在和项目数量统计；不包含截图、项目路径、窗口标题或账号凭据。</p>
        <Button variant="outline" @click="exportDiagnosticFile">
          <Download class="h-4 w-4" />导出诊断信息
        </Button>
        <p class="text-xs">
          自动更新与 Windows 代码签名需要发布方提供更新端点、更新签名密钥及代码签名证书，当前构建不会伪造这些安全材料。
        </p>
      </CardContent>
    </Card>

    <Card>
      <CardHeader><CardTitle class="text-sm">关于</CardTitle></CardHeader>
      <CardContent class="space-y-1.5 text-sm text-muted-foreground">
        <p>ImageSitter v1.0.0 — 图像特征标记、校准与离线回归工具。</p>
        <p>窗口捕获能力由 winsitter SDK 提供（后台捕获，不抢前台）。</p>
        <p>项目格式 v4 支持尺寸/DPI 适配、邻域与缩放搜索、多对象期望和外置回放样本。</p>
      </CardContent>
    </Card>
  </div>
</template>
