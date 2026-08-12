<script setup lang="ts">
/**
 * 校准页：采集同一形态的正样本波动，并用负样本评估区分度，
 * 自动推导每个特征点的参考颜色与容差。
 */
import { Camera, Eraser, FlaskConical, Sparkles, Wand2 } from "lucide-vue-next";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";
import Badge from "@/components/ui/badge/Badge.vue";
import Button from "@/components/ui/button/Button.vue";
import Select from "@/components/ui/select/Select.vue";
import type { SelectOption } from "@/components/ui/select/Select.vue";
import Slider from "@/components/ui/slider/Slider.vue";
import { loadSamplePng, samplePoints, suggestFeaturePoints, suggestTolerances } from "@/lib/ipc";
import { frameSizeCompatible, resolveObjectForFrame } from "@/lib/matching";
import type { FeatureCandidate, PointSuggestion, ReplayCase } from "@/lib/types";
import { rgbaToHex } from "@/lib/utils";
import { useProjectStore } from "@/stores/project";
import { useTargetStore } from "@/stores/target";

const projectStore = useProjectStore();
const targetStore = useTargetStore();

type Rgba = [number, number, number, number];

const positiveSamples = ref<Rgba[][]>([]);
const negativeSamples = ref<Rgba[][]>([]);
const margin = ref([12]);
const suggestions = ref<PointSuggestion[] | null>(null);
const collecting = ref(false);
const discovering = ref(false);
const discoveredCandidates = ref<FeatureCandidate[]>([]);
const rejectedCount = computed(
  () => suggestions.value?.filter((item) => !item.recommendKeep).length ?? 0,
);

const object = computed(() => projectStore.selectedObject);
const group = computed(() => projectStore.selectedGroup);
const coordinateMismatch = computed(() => {
  const target = projectStore.project.target;
  return !frameSizeCompatible(
    object.value,
    target.frameWidth,
    target.frameHeight,
    targetStore.frameWidth,
    targetStore.frameHeight,
  );
});
const sampleRows = computed(() => [
  ...positiveSamples.value.map((colors, index) => ({
    key: `positive-${index}`,
    label: `正 #${index + 1}`,
    positive: true,
    colors,
  })),
  ...negativeSamples.value.map((colors, index) => ({
    key: `negative-${index}`,
    label: `负 #${index + 1}`,
    positive: false,
    colors,
  })),
]);

const objectOptions = computed<SelectOption[]>(() =>
  projectStore.project.objects.map((item) => ({
    value: item.id,
    label: item.name,
  })),
);

const groupOptions = computed<SelectOption[]>(() =>
  (object.value?.groups ?? []).map((item) => ({
    value: item.id,
    label: item.name,
  })),
);

/** 采集一次样本：对组内所有点在最新帧上取色 */
async function collectSample(kind: "positive" | "negative") {
  if (!targetStore.bound) {
    toast.error("请先在捕获页绑定目标窗口");
    return;
  }
  if (!object.value || !group.value) {
    toast.error("请先选择对象和特征组");
    return;
  }
  if (!group.value.points.length) {
    toast.error("该组还没有特征点，请先在捕获页取色添加");
    return;
  }
  if (coordinateMismatch.value) {
    toast.error("当前帧尺寸与项目基准不一致，已停止采样以避免坐标误判");
    return;
  }
  collecting.value = true;
  try {
    const target = projectStore.project.target;
    const resolved = resolveObjectForFrame(
      object.value,
      target.frameWidth,
      target.frameHeight,
      targetStore.frameWidth,
      targetStore.frameHeight,
    );
    const resolvedGroup = resolved.groups.find((item) => item.id === group.value!.id)!;
    const absolute = resolvedGroup.points.map((point) => ({
      x: resolved.region.x + point.dx,
      y: resolved.region.y + point.dy,
    }));
    const colors = await samplePoints(targetStore.bound.targetId, absolute);
    const sample: Rgba[] = [];
    for (const color of colors) {
      if (!color) {
        toast.error("有采样点超出画面范围，请检查区域设置");
        return;
      }
      sample.push(color);
    }
    const bucket = kind === "positive" ? positiveSamples.value : negativeSamples.value;
    bucket.push(sample);
    suggestions.value = null;
    toast.success(`已采集第 ${bucket.length} 张${kind === "positive" ? "正" : "负"}样本`);
  } catch (error) {
    toast.error(String(error));
  } finally {
    collecting.value = false;
  }
}

async function compute() {
  if (!group.value) return;
  if (positiveSamples.value.length < 2) {
    toast.error("至少采集 2 张同一形态在不同动画帧下的正样本");
    return;
  }
  try {
    suggestions.value = await suggestTolerances(
      positiveSamples.value,
      negativeSamples.value,
      group.value.points.length,
      margin.value[0],
    );
  } catch (error) {
    toast.error(String(error));
  }
}

function applySuggestions() {
  if (!object.value || !group.value || !suggestions.value) return;
  projectStore.applySuggestions(
    object.value.id,
    group.value.id,
    suggestions.value,
  );
  toast.success("校准结果已应用到特征组");
}

function clearSamples() {
  positiveSamples.value = [];
  negativeSamples.value = [];
  suggestions.value = null;
  discoveredCandidates.value = [];
}

async function casePng(item: ReplayCase): Promise<string> {
  if (item.storage === "embedded") return item.pngDataUrl;
  if (!projectStore.filePath) throw new Error("外部样本需要已保存项目路径");
  return (await loadSamplePng(projectStore.filePath, item.relativePath, item.sha256)).pngDataUrl;
}

async function discoverFromReplay() {
  if (!object.value || !group.value) return;
  const positiveCases = projectStore.project.replayCases.filter((item) =>
    item.expectations.some(
      (expectation) =>
        expectation.objectId === object.value!.id &&
        expectation.expectedGroupId === group.value!.id,
    ),
  );
  const negativeCases = projectStore.project.replayCases.filter((item) =>
    item.expectations.some(
      (expectation) => expectation.objectId === object.value!.id && expectation.expectedGroupId == null,
    ),
  );
  const sizeCounts = new Map<string, number>();
  for (const item of positiveCases) {
    const key = `${item.width}x${item.height}`;
    sizeCounts.set(key, (sizeCounts.get(key) ?? 0) + 1);
  }
  const bestSize = [...sizeCounts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  if (!bestSize) {
    toast.error("回放集中没有当前视觉状态的正样本");
    return;
  }
  const [width, height] = bestSize.split("x").map(Number);
  const positives = positiveCases.filter((item) => item.width === width && item.height === height).slice(0, 200);
  const negatives = negativeCases.filter((item) => item.width === width && item.height === height).slice(0, 200);
  if (positives.length < 2 || negatives.length < 1) {
    toast.error(`尺寸 ${width}×${height} 下至少需要 2 张正样本和 1 张负样本`);
    return;
  }
  discovering.value = true;
  try {
    const target = projectStore.project.target;
    const resolved = resolveObjectForFrame(object.value, target.frameWidth, target.frameHeight, width, height);
    discoveredCandidates.value = await suggestFeaturePoints(
      await Promise.all(positives.map(casePng)),
      await Promise.all(negatives.map(casePng)),
      resolved.region,
      24,
      Math.max(4, Math.round(Math.min(resolved.region.w, resolved.region.h) / 18)),
    );
    toast.success(`从 ${positives.length} 张正样本和 ${negatives.length} 张负样本发现 ${discoveredCandidates.value.length} 个候选点`);
  } catch (error) {
    toast.error(String(error));
  } finally {
    discovering.value = false;
  }
}

function applyDiscoveredCandidates() {
  if (!object.value || !group.value || !discoveredCandidates.value.length) return;
  const target = projectStore.project.target;
  const firstPositive = projectStore.project.replayCases.find((item) =>
    item.expectations.some((expectation) => expectation.objectId === object.value!.id && expectation.expectedGroupId === group.value!.id),
  );
  if (!firstPositive) return;
  const sx = object.value.coordinateMode === "scale" && target.frameWidth > 0 ? target.frameWidth / firstPositive.width : 1;
  const sy = object.value.coordinateMode === "scale" && target.frameHeight > 0 ? target.frameHeight / firstPositive.height : 1;
  const candidates = discoveredCandidates.value.map((item) => ({
    ...item,
    dx: Math.min(object.value!.region.w - 1, Math.max(0, Math.round(item.dx * sx))),
    dy: Math.min(object.value!.region.h - 1, Math.max(0, Math.round(item.dy * sy))),
  }));
  const added = projectStore.addPointsBatch(object.value.id, group.value.id, candidates);
  toast.success(`已添加 ${added} 个跨样本高区分度特征点`);
}

function applyAndClean() {
  if (!object.value || !group.value || !suggestions.value) return;
  projectStore.applySuggestions(object.value.id, group.value.id, suggestions.value);
  const removed = projectStore.removeRejectedPoints(
    object.value.id,
    group.value.id,
    suggestions.value,
  );
  suggestions.value = null;
  positiveSamples.value = [];
  negativeSamples.value = [];
  toast.success(`已应用建议并移除 ${removed} 个低质量点`);
}

watch(
  () => [projectStore.selectedObjectId, projectStore.selectedGroupId],
  clearSamples,
);
</script>

<template>
  <div class="mx-auto max-w-4xl p-6">
    <h1 class="mb-1 text-lg font-semibold">校准特征容差</h1>
    <p class="mb-5 text-sm text-muted-foreground">
      一个特征组只对应一种形态。请采集该形态在不同动画帧、光照或蒙版下的正样本，
      再在对象缺失或相似对象出现时采集负样本；系统会推导容差并报告潜在误命中。
    </p>

    <div class="mb-4 grid grid-cols-2 gap-3">
      <div>
        <div class="mb-1.5 text-xs text-muted-foreground">对象</div>
        <Select
          :model-value="projectStore.selectedObjectId ?? undefined"
          :options="objectOptions"
          placeholder="选择对象"
          @update:model-value="projectStore.selectObject($event)"
        />
      </div>
      <div>
        <div class="mb-1.5 text-xs text-muted-foreground">特征组（形态组）</div>
        <Select
          :model-value="projectStore.selectedGroupId ?? undefined"
          :options="groupOptions"
          placeholder="选择特征组"
          @update:model-value="projectStore.selectGroup($event)"
        />
      </div>
    </div>

    <div class="mb-4 flex items-center gap-2">
      <Button
        :disabled="collecting || !targetStore.bound || coordinateMismatch"
        @click="collectSample('positive')"
      >
        <Camera class="h-4 w-4" />
        采集正样本
      </Button>
      <Button
        variant="outline"
        :disabled="collecting || !targetStore.bound || coordinateMismatch"
        @click="collectSample('negative')"
      >
        <Camera class="h-4 w-4" />
        采集负样本
      </Button>
      <Button variant="outline" :disabled="!sampleRows.length" @click="clearSamples">
        <Eraser class="h-4 w-4" />
        清空样本
      </Button>
      <div class="flex-1" />
      <div class="flex w-56 items-center gap-2">
        <span class="text-xs text-muted-foreground">安全边距</span>
        <Slider v-model="margin" :min="0" :max="60" class="flex-1" />
        <span class="w-6 text-right text-xs tabular-nums">
          {{ margin[0] }}
        </span>
      </div>
    </div>

    <div v-if="!targetStore.bound" class="mb-4 rounded-md border p-3 text-sm text-muted-foreground">
      尚未绑定目标窗口：请先到捕获页绑定，采集会在最新帧上自动取色。
    </div>
    <div v-else-if="coordinateMismatch" class="mb-4 rounded-md border border-destructive p-3 text-sm text-destructive">
      当前帧尺寸与项目基准不一致，校准已禁用。
    </div>

    <!-- 样本表 -->
    <div v-if="sampleRows.length" class="mb-5 overflow-auto rounded-lg border">
      <table class="w-full text-sm">
        <thead>
          <tr class="border-b text-left text-xs text-muted-foreground">
            <th class="px-3 py-2">样本</th>
            <th
              v-for="(point, index) in group?.points ?? []"
              :key="index"
              class="px-3 py-2"
            >
              点{{ index + 1 }} ({{ point.dx }},{{ point.dy }})
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="sample in sampleRows" :key="sample.key" class="border-b last:border-b-0">
            <td class="px-3 py-1.5 text-xs">
              <Badge :variant="sample.positive ? 'success' : 'secondary'" class="text-[10px]">
                {{ sample.label }}
              </Badge>
            </td>
            <td v-for="(color, pIndex) in sample.colors" :key="pIndex" class="px-3 py-1.5">
              <span class="flex items-center gap-1.5">
                <span
                  class="inline-block h-5 w-5 rounded border"
                  :style="{ background: rgbaToHex(color) }"
                />
                <span class="text-xs tabular-nums text-muted-foreground">
                  {{ rgbaToHex(color) }}
                </span>
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div
      v-else
      class="mb-5 flex items-center gap-3 rounded-lg border border-dashed p-6 text-sm text-muted-foreground"
    >
      <FlaskConical class="h-8 w-8 opacity-40" />
      <div>
        <p>还没有样本。让对象保持当前组对应的形态，先采集正样本。</p>
        <p class="mt-0.5 text-xs">建议至少 3 张正样本，并补充对象缺失时的负样本。</p>
      </div>
    </div>

    <!-- 建议 -->
    <div class="flex items-center gap-2">
      <Button :disabled="positiveSamples.length < 2" @click="compute">
        <Wand2 class="h-4 w-4" />
        计算容差建议
      </Button>
      <Button
        variant="default"
        :disabled="!suggestions"
        @click="applySuggestions"
      >
        应用到特征组
      </Button>
      <Button variant="outline" :disabled="discovering || !object || !group" @click="discoverFromReplay">
        <Sparkles class="h-4 w-4" />
        {{ discovering ? "分析中…" : "从回放集智能选点" }}
      </Button>
      <Button
        variant="destructive"
        :disabled="!suggestions || rejectedCount === 0"
        @click="applyAndClean"
      >
        应用并移除 {{ rejectedCount }} 个低质量点
      </Button>
    </div>

    <div v-if="suggestions" class="mt-4 overflow-auto rounded-lg border">
      <table class="w-full text-sm">
        <thead>
          <tr class="border-b text-left text-xs text-muted-foreground">
            <th class="px-3 py-2">点</th>
            <th class="px-3 py-2">建议参考色</th>
            <th class="px-3 py-2">建议容差 R/G/B</th>
            <th class="px-3 py-2">观测区间</th>
            <th class="px-3 py-2">alpha</th>
            <th class="px-3 py-2">质量分</th>
            <th class="px-3 py-2">提示</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="suggestion in suggestions"
            :key="suggestion.index"
            class="border-b last:border-b-0"
          >
            <td class="px-3 py-1.5 text-xs">{{ suggestion.index + 1 }}</td>
            <td class="px-3 py-1.5">
              <span class="flex items-center gap-1.5">
                <span
                  class="inline-block h-5 w-5 rounded border"
                  :style="{ background: rgbaToHex(suggestion.reference) }"
                />
                <span class="text-xs tabular-nums">
                  {{ rgbaToHex(suggestion.reference) }}
                </span>
              </span>
            </td>
            <td class="px-3 py-1.5 text-xs tabular-nums">
              {{ suggestion.tolerance.join(" / ") }}
            </td>
            <td class="px-3 py-1.5 text-xs tabular-nums text-muted-foreground">
              {{ rgbaToHex(suggestion.minObserved) }} ~
              {{ rgbaToHex(suggestion.maxObserved) }}
            </td>
            <td class="px-3 py-1.5">
              <Badge
                :variant="suggestion.alphaStable ? 'secondary' : 'outline'"
                class="text-[10px]"
              >
                 {{ suggestion.suggestedAlphaMode === "match"
                   ? `参与 ±${suggestion.alphaTolerance}`
                   : suggestion.alphaOpaque
                     ? "不透明，忽略"
                     : `波动 ${suggestion.alphaRange}，忽略` }}
              </Badge>
            </td>
            <td class="px-3 py-1.5">
              <Badge
                :variant="suggestion.recommendKeep ? 'success' : 'destructive'"
                class="text-[10px] tabular-nums"
              >
                {{ suggestion.qualityScore }}/100
              </Badge>
            </td>
            <td class="px-3 py-1.5 text-xs">
              <span :class="suggestion.recommendKeep ? 'text-muted-foreground' : 'text-destructive'">
                {{ suggestion.qualityReason }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="discoveredCandidates.length" class="mt-4 rounded-lg border p-3">
      <div class="mb-2 flex items-center gap-2">
        <div class="text-sm font-medium">跨样本候选点</div>
        <Badge variant="secondary">{{ discoveredCandidates.length }} 个</Badge>
        <div class="flex-1" />
        <Button size="sm" @click="applyDiscoveredCandidates">添加到当前视觉状态</Button>
      </div>
      <div class="grid grid-cols-2 gap-1 md:grid-cols-4">
        <div v-for="item in discoveredCandidates" :key="`${item.dx}:${item.dy}`" class="rounded bg-muted/50 p-1.5 text-[11px]">
          ({{ item.dx }},{{ item.dy }}) · 质量 {{ item.qualityScore }}<br />
          稳定波动 {{ item.positiveRange }} · 负样本距离 {{ item.negativeDistance }}
        </div>
      </div>
    </div>
  </div>
</template>
