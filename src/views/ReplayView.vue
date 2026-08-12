<script setup lang="ts">
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import {
  Camera,
  CheckCircle2,
  FileDown,
  FolderInput,
  Image as ImageIcon,
  Pause,
  Play,
  Tags,
  Trash2,
  Video,
  XCircle,
} from "lucide-vue-next";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";
import Badge from "@/components/ui/badge/Badge.vue";
import Button from "@/components/ui/button/Button.vue";
import Input from "@/components/ui/input/Input.vue";
import Select from "@/components/ui/select/Select.vue";
import type { SelectOption } from "@/components/ui/select/Select.vue";
import {
  captureFramePng,
  importSamplePng,
  listPngFiles,
  loadSamplePng,
  runMatchPngAdvanced,
  saveTextFile,
  storeSamplePngData,
} from "@/lib/ipc";
import { frameSizeCompatible, resolveObjectForFrame } from "@/lib/matching";
import type { MatchReport, ReplayCase, ReplayExpectation } from "@/lib/types";
import { rgbaToHex } from "@/lib/utils";
import { useProjectStore } from "@/stores/project";
import { useTargetStore } from "@/stores/target";

type OutcomeKind = "passed" | "falsePositive" | "falseNegative" | "stateError" | "error";
type ObjectOutcome = {
  objectId: string;
  expectedGroupId: string | null;
  kind: OutcomeKind;
  report: MatchReport | null;
  message: string;
};
type CaseOutcome = {
  status: "running" | "passed" | "failed" | "error";
  objects: ObjectOutcome[];
  message: string;
  elapsedMs: number;
};

const projectStore = useProjectStore();
const targetStore = useTargetStore();
const selectedCaseId = ref<string | null>(null);
const selectedIds = ref<string[]>([]);
const draftExpected = ref<Record<string, string>>({});
const storageMode = ref<"embedded" | "external">("external");
const frameCount = ref("10");
const intervalMs = ref("150");
const tagFilter = ref("");
const batchTags = ref("");
const recording = ref(false);
const importing = ref(false);
const running = ref(false);
const cancelRequested = ref(false);
const operationProgress = ref({ completed: 0, total: 0 });
const outcomes = ref<Record<string, CaseOutcome>>({});
const externalImages = ref<Record<string, string>>({});

const stateOptions = (objectId: string): SelectOption[] => {
  const object = projectStore.project.objects.find((item) => item.id === objectId);
  return [
    { value: "ignore", label: "不标注此对象" },
    { value: "absent", label: "对象不存在" },
    ...(object?.groups ?? []).map((group) => ({
      value: group.id,
      label: `${group.name}${group.enabled ? "" : "（已停用）"}`,
    })),
  ];
};
const visibleCases = computed(() => {
  const needle = tagFilter.value.trim().toLowerCase();
  if (!needle) return projectStore.project.replayCases;
  return projectStore.project.replayCases.filter(
    (item) =>
      item.name.toLowerCase().includes(needle) ||
      item.tags.some((tag) => tag.toLowerCase().includes(needle)),
  );
});
const selectedCase = computed(() =>
  projectStore.project.replayCases.find((item) => item.id === selectedCaseId.value) ?? null,
);
const selectedOutcome = computed(() =>
  selectedCase.value ? outcomes.value[selectedCase.value.id] ?? null : null,
);
const objectOutcomes = computed(() =>
  visibleCases.value.flatMap((item) => outcomes.value[item.id]?.objects ?? []),
);
const passedCount = computed(() => objectOutcomes.value.filter((item) => item.kind === "passed").length);
const falsePositiveCount = computed(() => objectOutcomes.value.filter((item) => item.kind === "falsePositive").length);
const falseNegativeCount = computed(() => objectOutcomes.value.filter((item) => item.kind === "falseNegative").length);
const stateErrorCount = computed(() => objectOutcomes.value.filter((item) => item.kind === "stateError").length);
const accuracy = computed(() =>
  objectOutcomes.value.length ? Math.round((passedCount.value / objectOutcomes.value.length) * 100) : 0,
);
const progressPercent = computed(() =>
  operationProgress.value.total
    ? Math.round((operationProgress.value.completed / operationProgress.value.total) * 100)
    : 0,
);

watch(
  () => projectStore.project.objects.map((item) => item.id),
  (ids) => {
    const next: Record<string, string> = {};
    for (const id of ids) next[id] = draftExpected.value[id] ?? "ignore";
    draftExpected.value = next;
  },
  { immediate: true },
);
watch(selectedCase, (item) => {
  if (item) void ensureImage(item);
});
watch(
  () => projectStore.project.objects,
  () => { outcomes.value = {}; },
  { deep: true },
);

function clampInteger(value: string, min: number, max: number, fallback: number) {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? Math.min(max, Math.max(min, parsed)) : fallback;
}

function draftExpectations(): ReplayExpectation[] {
  return projectStore.project.objects.flatMap((object) => {
    const expected = draftExpected.value[object.id] ?? "ignore";
    return expected === "ignore"
      ? []
      : [{ objectId: object.id, expectedGroupId: expected === "absent" ? null : expected }];
  });
}

function validateDraft(): ReplayExpectation[] | null {
  const expectations = draftExpectations();
  if (!expectations.length) {
    toast.error("请至少为一个对象设置预期状态或“不存在”");
    return null;
  }
  for (const expectation of expectations) {
    const object = projectStore.project.objects.find((item) => item.id === expectation.objectId)!;
    if (expectation.expectedGroupId && !object.groups.find((item) => item.id === expectation.expectedGroupId)?.enabled) {
      toast.error(`${object.name} 的预期状态已停用`);
      return null;
    }
  }
  if (storageMode.value === "external" && !projectStore.filePath) {
    toast.error("使用外部样本库前请先保存项目");
    return null;
  }
  return expectations;
}

async function normalizeCapture(pngDataUrl: string) {
  return storeSamplePngData(projectStore.filePath, pngDataUrl, storageMode.value);
}

async function collectFrame() {
  const expectations = validateDraft();
  if (!expectations || !targetStore.bound) {
    if (!targetStore.bound) toast.error("请先在捕获页绑定目标窗口");
    return;
  }
  recording.value = true;
  try {
    const frame = await captureFramePng(targetStore.bound.targetId);
    const stored = await normalizeCapture(frame.pngDataUrl);
    const item = projectStore.addReplayCase({
      name: `测试帧 ${projectStore.project.replayCases.length + 1}`,
      width: stored.width,
      height: stored.height,
      storage: storageMode.value,
      pngDataUrl: stored.pngDataUrl,
      relativePath: stored.relativePath,
      sha256: stored.sha256,
      expectations,
      tags: [],
    });
    selectedCaseId.value = item.id;
    if (stored.pngDataUrl) externalImages.value[item.id] = stored.pngDataUrl;
    toast.success("当前帧已加入多对象回放集");
  } catch (error) {
    toast.error(String(error));
  } finally {
    recording.value = false;
  }
}

async function recordFrames() {
  const expectations = validateDraft();
  const bound = targetStore.bound;
  if (!expectations || !bound || recording.value) {
    if (!bound) toast.error("请先在捕获页绑定目标窗口");
    return;
  }
  const count = clampInteger(frameCount.value, 1, 300, 10);
  const delay = clampInteger(intervalMs.value, 50, 5000, 150);
  frameCount.value = String(count);
  intervalMs.value = String(delay);
  recording.value = true;
  cancelRequested.value = false;
  operationProgress.value = { completed: 0, total: count };
  const pending: Omit<ReplayCase, "id" | "capturedAt">[] = [];
  try {
    for (let index = 0; index < count && !cancelRequested.value; index += 1) {
      const frame = await captureFramePng(bound.targetId);
      const stored = await normalizeCapture(frame.pngDataUrl);
      pending.push({
        name: `录制 ${new Date().toLocaleTimeString()} · ${index + 1}`,
        width: stored.width,
        height: stored.height,
        storage: storageMode.value,
        pngDataUrl: stored.pngDataUrl,
        relativePath: stored.relativePath,
        sha256: stored.sha256,
        expectations: structuredClone(expectations),
        tags: ["录制"],
      });
      operationProgress.value.completed = index + 1;
      if (index < count - 1 && !cancelRequested.value) {
        await new Promise((resolve) => setTimeout(resolve, delay));
      }
    }
    const added = projectStore.addReplayCasesBatch(pending);
    selectedCaseId.value = added[0]?.id ?? null;
    toast.success(`已保存 ${added.length} 帧${cancelRequested.value ? "（已取消后续录制）" : ""}`);
  } catch (error) {
    toast.error(String(error));
  } finally {
    recording.value = false;
    cancelRequested.value = false;
  }
}

async function importDirectory() {
  const expectations = validateDraft();
  if (!expectations || importing.value) return;
  const directory = await open({ title: "选择 PNG 样本目录", directory: true, multiple: false });
  if (typeof directory !== "string") return;
  importing.value = true;
  cancelRequested.value = false;
  const pending: Omit<ReplayCase, "id" | "capturedAt">[] = [];
  try {
    const files = await listPngFiles(directory);
    operationProgress.value = { completed: 0, total: files.length };
    for (const [index, path] of files.entries()) {
      if (cancelRequested.value) break;
      const sample = await importSamplePng(projectStore.filePath, path, storageMode.value);
      pending.push({
        name: path.split(/[\\/]/).pop() ?? `导入样本 ${index + 1}`,
        width: sample.width,
        height: sample.height,
        storage: storageMode.value,
        pngDataUrl: sample.pngDataUrl,
        relativePath: sample.relativePath,
        sha256: sample.sha256,
        expectations: structuredClone(expectations),
        tags: ["导入"],
      });
      operationProgress.value.completed = index + 1;
    }
    const added = projectStore.addReplayCasesBatch(pending);
    selectedCaseId.value = added[0]?.id ?? null;
    toast.success(`已导入 ${added.length} 张 PNG`);
  } catch (error) {
    toast.error(String(error));
  } finally {
    importing.value = false;
    cancelRequested.value = false;
  }
}

async function ensureImage(item: ReplayCase): Promise<string> {
  if (item.storage === "embedded") return item.pngDataUrl;
  if (externalImages.value[item.id]) return externalImages.value[item.id];
  if (!projectStore.filePath) throw new Error("外部样本缺少项目路径");
  const loaded = await loadSamplePng(projectStore.filePath, item.relativePath, item.sha256);
  externalImages.value = { ...externalImages.value, [item.id]: loaded.pngDataUrl };
  return loaded.pngDataUrl;
}

function expectationLabel(expectation: ReplayExpectation): string {
  const object = projectStore.project.objects.find((item) => item.id === expectation.objectId);
  if (!object) return "对象已删除";
  if (!expectation.expectedGroupId) return `${object.name}：不存在`;
  return `${object.name}：${object.groups.find((item) => item.id === expectation.expectedGroupId)?.name ?? "状态已删除"}`;
}

async function evaluateExpectation(
  pngDataUrl: string,
  frame: ReplayCase,
  expectation: ReplayExpectation,
): Promise<ObjectOutcome> {
  const object = projectStore.project.objects.find((item) => item.id === expectation.objectId);
  if (!object) return { ...expectation, kind: "error", report: null, message: "对象已删除" };
  const target = projectStore.project.target;
  if (!frameSizeCompatible(object, target.frameWidth, target.frameHeight, frame.width, frame.height)) {
    return { ...expectation, kind: "error", report: null, message: "固定坐标对象与样本尺寸不兼容" };
  }
  try {
    const resolved = resolveObjectForFrame(
      object,
      target.frameWidth,
      target.frameHeight,
      frame.width,
      frame.height,
    );
    const report = await runMatchPngAdvanced(
      pngDataUrl,
      resolved.region,
      resolved.groups,
      object.searchRadius,
      object.scaleSearchPercent,
    );
    const matched = report.groups.filter((item) => item.matched).map((item) => item.id);
    if (!expectation.expectedGroupId) {
      return {
        ...expectation,
        kind: report.matched ? "falsePositive" : "passed",
        report,
        message: report.matched ? "对象本应不存在，但规则发生命中" : "正确未命中",
      };
    }
    if (!report.matched) {
      return { ...expectation, kind: "falseNegative", report, message: "预期对象存在，但没有状态命中" };
    }
    if (matched.length !== 1 || matched[0] !== expectation.expectedGroupId) {
      const names = object.groups.filter((item) => matched.includes(item.id)).map((item) => item.name);
      return { ...expectation, kind: "stateError", report, message: `状态错误或歧义：${names.join("、") || "无"}` };
    }
    return { ...expectation, kind: "passed", report, message: "状态识别正确" };
  } catch (error) {
    return { ...expectation, kind: "error", report: null, message: String(error) };
  }
}

async function runCase(item: ReplayCase): Promise<CaseOutcome> {
  const started = performance.now();
  try {
    const pngDataUrl = await ensureImage(item);
    const objects = await Promise.all(
      item.expectations.map((expectation) => evaluateExpectation(pngDataUrl, item, expectation)),
    );
    const hasError = objects.some((entry) => entry.kind === "error");
    const passed = objects.length > 0 && objects.every((entry) => entry.kind === "passed");
    return {
      status: hasError ? "error" : passed ? "passed" : "failed",
      objects,
      message: passed ? "全部对象符合预期" : `${objects.filter((entry) => entry.kind !== "passed").length} 个对象不符合预期`,
      elapsedMs: performance.now() - started,
    };
  } catch (error) {
    return { status: "error", objects: [], message: String(error), elapsedMs: performance.now() - started };
  }
}

async function runOne(item: ReplayCase) {
  outcomes.value = { ...outcomes.value, [item.id]: { status: "running", objects: [], message: "匹配中…", elapsedMs: 0 } };
  const result = await runCase(item);
  outcomes.value = { ...outcomes.value, [item.id]: result };
  selectedCaseId.value = item.id;
  return result;
}

async function runAll() {
  if (!visibleCases.value.length || running.value) return;
  running.value = true;
  cancelRequested.value = false;
  operationProgress.value = { completed: 0, total: visibleCases.value.length };
  let nextIndex = 0;
  const cases = [...visibleCases.value];
  const worker = async () => {
    while (!cancelRequested.value) {
      const index = nextIndex++;
      const item = cases[index];
      if (!item) return;
      await runOne(item);
      operationProgress.value.completed += 1;
    }
  };
  try {
    await Promise.all(Array.from({ length: Math.min(3, cases.length) }, worker));
    toast.success(`回放完成：准确率 ${accuracy.value}%${cancelRequested.value ? "（已取消剩余任务）" : ""}`);
  } finally {
    running.value = false;
    cancelRequested.value = false;
  }
}

function toggleSelected(id: string, checked: boolean) {
  selectedIds.value = checked
    ? [...new Set([...selectedIds.value, id])]
    : selectedIds.value.filter((item) => item !== id);
}

async function removeSelected() {
  if (!selectedIds.value.length) return;
  const approved = await confirm(`确定移除选中的 ${selectedIds.value.length} 个测试帧吗？外部图片文件将保留，可稍后清理。`, {
    title: "批量移除测试帧",
    kind: "warning",
  });
  if (!approved) return;
  projectStore.removeReplayCases(selectedIds.value);
  selectedIds.value = [];
}

function applyDraftToSelected() {
  const expectations = draftExpectations();
  if (!expectations.length || !selectedIds.value.length) return;
  for (const id of selectedIds.value) projectStore.setReplayExpectations(id, structuredClone(expectations));
  toast.success(`已重新标记 ${selectedIds.value.length} 个测试帧`);
}

function applyTagsToSelected() {
  const tags = batchTags.value.split(/[,，]/).map((item) => item.trim()).filter(Boolean);
  for (const id of selectedIds.value) projectStore.setReplayTags(id, tags);
  toast.success(`已更新 ${selectedIds.value.length} 个测试帧的标签`);
}

function reportRows() {
  return visibleCases.value.flatMap((frame) => {
    const outcome = outcomes.value[frame.id];
    return (outcome?.objects ?? []).map((objectOutcome) => ({
      frameId: frame.id,
      frameName: frame.name,
      tags: frame.tags.join("|"),
      storage: frame.storage,
      objectId: objectOutcome.objectId,
      objectName: projectStore.project.objects.find((item) => item.id === objectOutcome.objectId)?.name ?? "",
      expectedGroupId: objectOutcome.expectedGroupId,
      result: objectOutcome.kind,
      message: objectOutcome.message,
      elapsedMs: outcome.elapsedMs,
      offsetX: objectOutcome.report?.offsetX ?? 0,
      offsetY: objectOutcome.report?.offsetY ?? 0,
      matchedScale: objectOutcome.report?.matchedScale ?? 1,
    }));
  });
}

async function exportReport(format: "json" | "csv" | "html") {
  const rows = reportRows();
  if (!rows.length) {
    toast.error("请先运行回放测试");
    return;
  }
  const path = await save({
    title: "导出回放报告",
    defaultPath: `imagesitter-report.${format}`,
    filters: [{ name: `${format.toUpperCase()} 报告`, extensions: [format] }],
  });
  if (!path) return;
  const escapeCsv = (value: unknown) => `"${String(value ?? "").replaceAll('"', '""')}"`;
  const escapeHtml = (value: unknown) => String(value ?? "").replace(/[&<>"']/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character]!);
  let content: string;
  if (format === "json") {
    content = JSON.stringify({ generatedAt: new Date().toISOString(), summary: { accuracy: accuracy.value, falsePositive: falsePositiveCount.value, falseNegative: falseNegativeCount.value, stateError: stateErrorCount.value }, rows }, null, 2);
  } else if (format === "csv") {
    const keys = Object.keys(rows[0]) as (keyof typeof rows[number])[];
    content = [keys.join(","), ...rows.map((row) => keys.map((key) => escapeCsv(row[key])).join(","))].join("\r\n");
  } else {
    const headers = ["frameName", "tags", "objectName", "result", "message", "elapsedMs", "offsetX", "offsetY", "matchedScale"] as const;
    content = `<!doctype html><meta charset="utf-8"><title>ImageSitter 回放报告</title><style>body{font-family:system-ui;margin:24px}table{border-collapse:collapse;width:100%}th,td{border:1px solid #ccc;padding:6px;text-align:left}th{background:#eee}.passed{color:green}.error,.falsePositive,.falseNegative,.stateError{color:#b91c1c}</style><h1>ImageSitter 回放报告</h1><p>准确率 ${accuracy.value}% · 漏检 ${falseNegativeCount.value} · 误检 ${falsePositiveCount.value} · 状态错 ${stateErrorCount.value}</p><table><thead><tr>${headers.map((key) => `<th>${key}</th>`).join("")}</tr></thead><tbody>${rows.map((row) => `<tr class="${escapeHtml(row.result)}">${headers.map((key) => `<td>${escapeHtml(row[key])}</td>`).join("")}</tr>`).join("")}</tbody></table>`;
  }
  await saveTextFile(path, content);
  toast.success("回放报告已导出");
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <section class="flex min-w-0 flex-1 flex-col">
      <div class="space-y-3 border-b p-4">
        <div class="flex items-start gap-2">
          <div>
            <h1 class="text-lg font-semibold">多对象回放测试</h1>
            <p class="text-xs text-muted-foreground">一张帧可标记多个对象；支持受管外部样本、并行回归、取消和报告。</p>
          </div>
          <div class="flex-1" />
          <template v-if="objectOutcomes.length">
            <Badge :variant="accuracy === 100 ? 'success' : 'destructive'">准确率 {{ accuracy }}%</Badge>
            <Badge variant="secondary">漏检 {{ falseNegativeCount }}</Badge>
            <Badge variant="secondary">误检 {{ falsePositiveCount }}</Badge>
            <Badge variant="secondary">状态错 {{ stateErrorCount }}</Badge>
          </template>
        </div>

        <div class="grid grid-cols-2 gap-x-3 gap-y-1 rounded-md border bg-muted/20 p-2 xl:grid-cols-4">
          <div v-for="object in projectStore.project.objects" :key="object.id" class="flex items-center gap-2">
            <span class="w-24 truncate text-xs" :title="object.name">{{ object.name }}</span>
            <Select v-model="draftExpected[object.id]" :options="stateOptions(object.id)" class="min-w-0 flex-1" />
          </div>
          <div v-if="!projectStore.project.objects.length" class="col-span-full text-xs text-muted-foreground">请先创建对象和视觉状态。</div>
        </div>

        <div class="flex flex-wrap items-end gap-2">
          <div class="w-36">
            <div class="mb-1 text-[11px] text-muted-foreground">图像存储</div>
            <Select v-model="storageMode" :options="[{ value: 'external', label: '外部样本库（推荐）' }, { value: 'embedded', label: '内嵌项目' }]" />
          </div>
          <Button :disabled="recording || importing || !targetStore.bound" @click="collectFrame"><Camera class="h-4 w-4" />单帧</Button>
          <div class="w-20"><div class="mb-1 text-[11px] text-muted-foreground">帧数 1～300</div><Input v-model="frameCount" class="h-9" /></div>
          <div class="w-24"><div class="mb-1 text-[11px] text-muted-foreground">间隔 ms</div><Input v-model="intervalMs" class="h-9" /></div>
          <Button variant="outline" :disabled="recording || importing || !targetStore.bound" @click="recordFrames"><Video class="h-4 w-4" />开始录制</Button>
          <Button variant="outline" :disabled="!recording && !importing && !running" @click="cancelRequested = true"><Pause class="h-4 w-4" />取消</Button>
          <Button variant="outline" :disabled="recording || importing" @click="importDirectory"><FolderInput class="h-4 w-4" />导入 PNG 目录</Button>
          <Button variant="outline" :disabled="running || !visibleCases.length" @click="runAll"><Play class="h-4 w-4" />并行运行</Button>
          <div v-if="recording || importing || running" class="min-w-36 text-xs text-muted-foreground">
            {{ operationProgress.completed }}/{{ operationProgress.total }}（{{ progressPercent }}%）
            <div class="mt-1 h-1.5 overflow-hidden rounded bg-muted"><div class="h-full bg-primary" :style="{ width: `${progressPercent}%` }" /></div>
          </div>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <Input v-model="tagFilter" class="h-8 w-44" placeholder="按名称/标签过滤" />
          <span class="text-xs text-muted-foreground">已选 {{ selectedIds.length }}</span>
          <Button size="sm" variant="outline" :disabled="!selectedIds.length" @click="applyDraftToSelected"><Tags class="h-3.5 w-3.5" />按上方期望重标</Button>
          <Input v-model="batchTags" class="h-8 w-40" placeholder="标签，逗号分隔" />
          <Button size="sm" variant="outline" :disabled="!selectedIds.length" @click="applyTagsToSelected">应用标签</Button>
          <Button size="sm" variant="destructive" :disabled="!selectedIds.length" @click="removeSelected"><Trash2 class="h-3.5 w-3.5" />批量移除</Button>
          <div class="flex-1" />
          <Button size="sm" variant="outline" @click="exportReport('json')"><FileDown class="h-3.5 w-3.5" />JSON</Button>
          <Button size="sm" variant="outline" @click="exportReport('csv')">CSV</Button>
          <Button size="sm" variant="outline" @click="exportReport('html')">HTML</Button>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-auto p-4">
        <div v-if="visibleCases.length" class="grid grid-cols-1 gap-2 xl:grid-cols-2">
          <button v-for="item in visibleCases" :key="item.id" class="flex gap-3 rounded-lg border bg-background p-2 text-left hover:border-primary cursor-pointer" :class="selectedCaseId === item.id && 'border-primary'" @click="selectedCaseId = item.id">
            <input type="checkbox" class="mt-1" :checked="selectedIds.includes(item.id)" @click.stop @change="toggleSelected(item.id, ($event.target as HTMLInputElement).checked)" />
            <div class="flex h-20 w-28 shrink-0 items-center justify-center overflow-hidden rounded border bg-black">
              <img v-if="item.pngDataUrl || externalImages[item.id]" :src="item.pngDataUrl || externalImages[item.id]" class="h-full w-full object-contain" />
              <ImageIcon v-else class="h-6 w-6 text-white/40" />
            </div>
            <div class="min-w-0 flex-1">
              <Input :model-value="item.name" class="h-7" @click.stop @change="projectStore.renameReplayCase(item.id, ($event.target as HTMLInputElement).value.trim())" />
              <div class="mt-1 flex flex-wrap gap-1"><Badge v-for="tag in item.tags" :key="tag" variant="secondary" class="text-[9px]">{{ tag }}</Badge><Badge variant="outline" class="text-[9px]">{{ item.storage === 'external' ? '外部' : '内嵌' }}</Badge></div>
              <div class="mt-1 truncate text-[11px] text-muted-foreground">{{ item.expectations.map(expectationLabel).join(" · ") || "无期望" }}</div>
              <div v-if="outcomes[item.id]" class="mt-1 text-xs" :class="outcomes[item.id].status === 'passed' ? 'text-emerald-600' : 'text-destructive'">{{ outcomes[item.id].message }} · {{ outcomes[item.id].elapsedMs.toFixed(1) }}ms</div>
            </div>
            <Button size="icon" variant="ghost" class="h-7 w-7" @click.stop="runOne(item)"><Play class="h-3.5 w-3.5" /></Button>
          </button>
        </div>
        <div v-else class="flex h-full items-center justify-center rounded-lg border border-dashed text-sm text-muted-foreground"><div class="text-center"><ImageIcon class="mx-auto mb-2 h-10 w-10 opacity-30" /><p>还没有符合筛选条件的测试帧</p></div></div>
      </div>
    </section>

    <aside class="w-80 shrink-0 overflow-auto border-l p-3">
      <div class="mb-3 text-sm font-semibold">逐对象诊断</div>
      <template v-if="selectedCase && selectedOutcome">
        <div v-for="entry in selectedOutcome.objects" :key="entry.objectId" class="mb-3 rounded-md border p-2">
          <div class="mb-2 flex items-center gap-1.5"><CheckCircle2 v-if="entry.kind === 'passed'" class="h-4 w-4 text-emerald-500" /><XCircle v-else class="h-4 w-4 text-destructive" /><span class="text-xs font-medium">{{ expectationLabel(entry) }}</span></div>
          <p class="mb-2 text-[11px] text-muted-foreground">{{ entry.message }}</p>
          <div v-if="entry.report" class="mb-1 text-[10px] text-muted-foreground">偏移 {{ entry.report.offsetX }},{{ entry.report.offsetY }} · 缩放 {{ Math.round(entry.report.matchedScale * 100) }}%</div>
          <div v-for="groupResult in entry.report?.groups ?? []" :key="groupResult.id" class="mb-2 rounded bg-muted/40 p-1.5">
            <div class="flex justify-between text-[11px] font-medium"><span>{{ projectStore.project.objects.find((item) => item.id === entry.objectId)?.groups.find((group) => group.id === groupResult.id)?.name }}</span><span>{{ groupResult.passedCount }}/{{ groupResult.required }}</span></div>
            <div v-for="point in groupResult.points.filter((item) => !item.ok)" :key="point.index" class="mt-1 text-[10px] text-destructive">点{{ point.index + 1 }} {{ point.reason }} · {{ rgbaToHex(point.actual) }} · 相似 {{ point.similarity }}%</div>
          </div>
        </div>
      </template>
      <div v-else class="py-8 text-center text-xs text-muted-foreground">运行测试帧后显示逐对象、逐状态和失败点原因。</div>
    </aside>
  </div>
</template>
