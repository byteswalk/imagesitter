<script setup lang="ts">
/**
 * 对象与特征页：管理对象、特征组与采样点，并对实时画面运行匹配验证。
 */
import { BookOpen, Copy, Play, Plus, Save, Square, Trash2, Wand2 } from "lucide-vue-next";
import { computed, onUnmounted, ref } from "vue";
import { toast } from "vue-sonner";
import Badge from "@/components/ui/badge/Badge.vue";
import Button from "@/components/ui/button/Button.vue";
import Input from "@/components/ui/input/Input.vue";
import Select from "@/components/ui/select/Select.vue";
import type { SelectOption } from "@/components/ui/select/Select.vue";
import Switch from "@/components/ui/switch/Switch.vue";
import { runMatchAdvanced } from "@/lib/ipc";
import { frameSizeCompatible, resolveObjectForFrame } from "@/lib/matching";
import type { MatchReport } from "@/lib/types";
import { rgbaToHex } from "@/lib/utils";
import { useProjectStore } from "@/stores/project";
import { useTargetStore } from "@/stores/target";

const projectStore = useProjectStore();
const targetStore = useTargetStore();

const report = ref<MatchReport | null>(null);
const testing = ref(false);
const autoTest = ref(false);
const selectedTemplateId = ref("");
let autoTimer: ReturnType<typeof setInterval> | null = null;

const alphaOptions: SelectOption[] = [
  { value: "ignore", label: "忽略 alpha" },
  { value: "match", label: "参与比对" },
];
const coordinateOptions: SelectOption[] = [
  { value: "fixed", label: "固定像素（严格）" },
  { value: "scale", label: "随客户区比例缩放" },
  { value: "anchor", label: "锚点定位（尺寸不变）" },
];
const anchorXOptions: SelectOption[] = [
  { value: "start", label: "左侧" },
  { value: "center", label: "水平居中" },
  { value: "end", label: "右侧" },
];
const anchorYOptions: SelectOption[] = [
  { value: "start", label: "顶部" },
  { value: "center", label: "垂直居中" },
  { value: "end", label: "底部" },
];
const templateOptions = computed<SelectOption[]>(() =>
  projectStore.templates.map((template) => ({ value: template.id, label: template.name })),
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

/** 双击改名：对象列表项 */
const editingObjectId = ref<string | null>(null);
const editingObjectName = ref("");

function startRenameObject(id: string, name: string) {
  editingObjectId.value = id;
  editingObjectName.value = name;
}

function commitRenameObject() {
  const name = editingObjectName.value.trim();
  if (editingObjectId.value && name) {
    projectStore.renameObject(editingObjectId.value, name);
  }
  editingObjectId.value = null;
}

/** 双击改名：形态组标签页 */
const editingGroupId = ref<string | null>(null);
const editingGroupName = ref("");

function startRenameGroup(id: string, name: string) {
  editingGroupId.value = id;
  editingGroupName.value = name;
}

function commitRenameGroup() {
  const name = editingGroupName.value.trim();
  if (object.value && editingGroupId.value && name) {
    projectStore.renameGroup(object.value.id, editingGroupId.value, name);
  }
  editingGroupId.value = null;
}

function parseColor(hex: string): [number, number, number, number] | null {
  const match = hex.trim().match(/^#?([0-9a-fA-F]{6})$/);
  if (!match) return null;
  const value = Number.parseInt(match[1], 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255, 255];
}

function clampNumber(value: string, max = 4095, min = 0): number {
  const parsed = Number.parseInt(value, 10);
  if (Number.isNaN(parsed)) return min;
  return Math.min(Math.max(parsed, min), max);
}

function saveTemplate() {
  if (!object.value) return;
  try {
    const template = projectStore.saveObjectTemplate(object.value.id, object.value.name);
    if (template) {
      selectedTemplateId.value = template.id;
      toast.success("对象模板已保存到本机");
    }
  } catch (error) {
    toast.error(String(error));
  }
}

function createFromTemplate() {
  if (!selectedTemplateId.value) return;
  if (projectStore.addObjectFromTemplate(selectedTemplateId.value)) {
    toast.success("已从模板创建对象");
  }
}

function deleteTemplate() {
  if (!selectedTemplateId.value) return;
  projectStore.removeObjectTemplate(selectedTemplateId.value);
  selectedTemplateId.value = projectStore.templates[0]?.id ?? "";
  toast.success("模板已删除");
}

/** 从当前区域的边缘和高对比位置自动挑选稀疏候选点，后续再用校准质量分筛选。 */
function autoGeneratePoints() {
  const currentObject = object.value;
  const currentGroup = group.value;
  const bitmap = targetStore.frameBitmap;
  if (!currentObject || !currentGroup || !bitmap) {
    toast.error("请先绑定窗口并等待预览帧");
    return;
  }
  if (coordinateMismatch.value) {
    toast.error("当前帧尺寸与项目基准不一致，不能自动选点");
    return;
  }
  const { x, y, w, h } = currentObject.region;
  if (w < 3 || h < 3 || w * h > 8_000_000) {
    toast.error("区域过小或像素数量超过 800 万，无法自动选点");
    return;
  }
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return;
  context.drawImage(bitmap, x, y, w, h, 0, 0, w, h);
  const pixels = context.getImageData(0, 0, w, h).data;
  const step = Math.max(1, Math.ceil(Math.sqrt((w * h) / 30_000)));
  const candidates: { dx: number; dy: number; score: number; reference: [number, number, number, number] }[] = [];
  const colorDifference = (first: number, second: number) =>
    Math.abs(pixels[first] - pixels[second]) +
    Math.abs(pixels[first + 1] - pixels[second + 1]) +
    Math.abs(pixels[first + 2] - pixels[second + 2]);
  for (let py = 1; py < h - 1; py += step) {
    for (let px = 1; px < w - 1; px += step) {
      const offset = (py * w + px) * 4;
      if (pixels[offset + 3] < 24) continue;
      const contrast =
        colorDifference(offset, offset - 4) +
        colorDifference(offset, offset + 4) +
        colorDifference(offset, offset - w * 4) +
        colorDifference(offset, offset + w * 4);
      const saturation = Math.max(pixels[offset], pixels[offset + 1], pixels[offset + 2]) -
        Math.min(pixels[offset], pixels[offset + 1], pixels[offset + 2]);
      candidates.push({
        dx: px,
        dy: py,
        score: contrast + saturation,
        reference: [pixels[offset], pixels[offset + 1], pixels[offset + 2], pixels[offset + 3]],
      });
    }
  }
  candidates.sort((a, b) => b.score - a.score);
  const occupied = currentGroup.points.map((point) => ({ dx: point.dx, dy: point.dy }));
  const chosen: typeof candidates = [];
  const minimumDistance = Math.max(5, Math.round(Math.min(w, h) / 16));
  for (const candidate of candidates) {
    if (candidate.score < 60) break;
    if (
      [...occupied, ...chosen].some(
        (point) => Math.hypot(point.dx - candidate.dx, point.dy - candidate.dy) < minimumDistance,
      )
    ) continue;
    chosen.push(candidate);
    if (chosen.length >= 16) break;
  }
  const added = projectStore.addPointsBatch(currentObject.id, currentGroup.id, chosen);
  if (added) {
    toast.success(`已补充 ${added} 个高对比候选点；建议到校准页用正/负样本复核质量分`);
  } else {
    toast.info("没有发现新的高质量候选点");
  }
}

function setReference(index: number, hex: string) {
  if (!object.value || !group.value) return;
  const rgba = parseColor(hex);
  if (!rgba) return;
  const current = group.value.points[index];
  projectStore.updatePoint(object.value.id, group.value.id, index, {
    reference: [rgba[0], rgba[1], rgba[2], current?.reference[3] ?? 255],
  });
}

async function runOnce() {
  if (testing.value) return;
  if (!targetStore.bound) {
    toast.error("请先在捕获页绑定目标窗口");
    return;
  }
  if (!object.value) {
    toast.error("请先选择对象");
    return;
  }
  if (coordinateMismatch.value) {
    toast.error("当前帧尺寸与项目基准不一致，已停止验证以避免坐标误判");
    return;
  }
  testing.value = true;
  try {
    const target = projectStore.project.target;
    const resolved = resolveObjectForFrame(
      object.value,
      target.frameWidth,
      target.frameHeight,
      targetStore.frameWidth,
      targetStore.frameHeight,
    );
    report.value = await runMatchAdvanced(
      targetStore.bound.targetId,
      resolved.region,
      resolved.groups,
      object.value.searchRadius,
      object.value.scaleSearchPercent,
    );
  } catch (error) {
    report.value = null;
    toast.error(String(error));
  } finally {
    testing.value = false;
  }
}

function toggleAutoTest() {
  autoTest.value = !autoTest.value;
  if (autoTest.value) {
    autoTimer = setInterval(() => void runOnce(), 500);
  } else if (autoTimer) {
    clearInterval(autoTimer);
    autoTimer = null;
  }
}

onUnmounted(() => {
  if (autoTimer) clearInterval(autoTimer);
});
</script>

<template>
  <div class="flex h-full min-h-0">
    <!-- 对象列表 -->
    <div class="flex w-52 shrink-0 flex-col border-r">
      <div class="flex items-center justify-between p-3 pb-2">
        <div class="text-sm font-semibold">对象</div>
        <Button
          size="icon"
          variant="ghost"
          class="h-7 w-7"
          @click="
            projectStore.addObject(
              `对象${projectStore.project.objects.length + 1}`,
            )
          "
        >
          <Plus class="h-4 w-4" />
        </Button>
      </div>
      <div class="min-h-0 flex-1 overflow-auto px-2 pb-2">
        <button
          v-for="item in projectStore.project.objects"
          :key="item.id"
          class="mb-1 flex w-full items-center justify-between rounded-md border bg-background px-2.5 py-2 text-left text-sm transition-colors hover:border-primary cursor-pointer"
          :class="{
            'border-primary': item.id === projectStore.selectedObjectId,
          }"
          title="双击改名"
          @click="projectStore.selectObject(item.id)"
          @dblclick.stop="startRenameObject(item.id, item.name)"
        >
          <input
            v-if="editingObjectId === item.id"
            v-model="editingObjectName"
            class="w-full min-w-0 rounded border bg-background px-1 text-sm"
            @click.stop
            @keydown.enter.prevent="commitRenameObject"
            @keydown.esc="editingObjectId = null"
            @blur="commitRenameObject"
          />
          <span v-else class="truncate">{{ item.name }}</span>
          <Badge variant="secondary" class="ml-1 text-[10px]">
            {{ item.groups.length }}组
          </Badge>
        </button>
        <div
          v-if="!projectStore.project.objects.length"
          class="px-2 py-6 text-center text-xs text-muted-foreground"
        >
          还没有对象，去捕获页框选区域会自动提示新建
        </div>
      </div>
      <div class="space-y-2 border-t p-2">
        <div class="flex items-center gap-1 text-[11px] font-medium text-muted-foreground">
          <BookOpen class="h-3.5 w-3.5" />
          对象模板（本机）
        </div>
        <Select
          v-if="templateOptions.length"
          :model-value="selectedTemplateId || templateOptions[0]?.value || ''"
          :options="templateOptions"
          @update:model-value="selectedTemplateId = $event"
        />
        <div class="flex gap-1">
          <Button size="sm" variant="outline" class="min-w-0 flex-1 px-2" :disabled="!object" @click="saveTemplate">
            <Save class="h-3.5 w-3.5" />
            存为模板
          </Button>
          <Button
            size="sm"
            variant="outline"
            class="px-2"
            :disabled="!templateOptions.length"
            title="从模板新建"
            @click="createFromTemplate"
          >
            <Plus class="h-3.5 w-3.5" />
          </Button>
          <Button
            size="sm"
            variant="ghost"
            class="px-2"
            :disabled="!templateOptions.length"
            title="删除模板"
            @click="deleteTemplate"
          >
            <Trash2 class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </div>

    <!-- 中间：组与点编辑 -->
    <div class="min-w-0 flex-1 overflow-auto p-4">
      <template v-if="object">
        <div class="mb-4 flex items-center gap-3">
          <Input
            :model-value="object.name"
            class="h-8 w-48"
            @update:model-value="projectStore.renameObject(object!.id, $event)"
          />
          <span class="text-xs text-muted-foreground">
            区域 x/y/w/h：
          </span>
          <Input
            :model-value="String(object.region.x)"
            class="h-8 w-16 tabular-nums"
            @update:model-value="
              projectStore.setRegion(object!.id, {
                ...object!.region,
                x: clampNumber($event),
              })
            "
          />
          <Input
            :model-value="String(object.region.y)"
            class="h-8 w-16 tabular-nums"
            @update:model-value="
              projectStore.setRegion(object!.id, {
                ...object!.region,
                y: clampNumber($event),
              })
            "
          />
          <Input
            :model-value="String(object.region.w)"
            class="h-8 w-16 tabular-nums"
            @update:model-value="
              projectStore.setRegion(object!.id, {
                ...object!.region,
                w: clampNumber($event, 4095, 1),
              })
            "
          />
          <Input
            :model-value="String(object.region.h)"
            class="h-8 w-16 tabular-nums"
            @update:model-value="
              projectStore.setRegion(object!.id, {
                ...object!.region,
                h: clampNumber($event, 4095, 1),
              })
            "
          />
          <div class="flex-1" />
          <Button
            size="sm"
            variant="destructive"
            @click="projectStore.removeObject(object!.id)"
          >
            <Trash2 class="h-3.5 w-3.5" />
            删除对象
          </Button>
        </div>

        <div class="mb-4 flex flex-wrap items-end gap-2 rounded-md border bg-muted/20 p-2.5">
          <div class="w-48">
            <div class="mb-1 text-[11px] text-muted-foreground">尺寸与 DPI 适配</div>
            <Select
              :model-value="object!.coordinateMode"
              :options="coordinateOptions"
              @update:model-value="projectStore.setObjectAdaptation(object!.id, { coordinateMode: $event as 'fixed' | 'scale' | 'anchor' })"
            />
          </div>
          <template v-if="object!.coordinateMode === 'anchor'">
            <div class="w-32">
              <div class="mb-1 text-[11px] text-muted-foreground">水平锚点</div>
              <Select
                :model-value="object!.anchorX"
                :options="anchorXOptions"
                @update:model-value="projectStore.setObjectAdaptation(object!.id, { anchorX: $event as 'start' | 'center' | 'end' })"
              />
            </div>
            <div class="w-32">
              <div class="mb-1 text-[11px] text-muted-foreground">垂直锚点</div>
              <Select
                :model-value="object!.anchorY"
                :options="anchorYOptions"
                @update:model-value="projectStore.setObjectAdaptation(object!.id, { anchorY: $event as 'start' | 'center' | 'end' })"
              />
            </div>
          </template>
          <div class="w-24">
            <div class="mb-1 text-[11px] text-muted-foreground">位置搜索 px</div>
            <Input
              :model-value="String(object!.searchRadius)"
              class="h-9"
              @update:model-value="projectStore.setObjectAdaptation(object!.id, { searchRadius: clampNumber($event, 32) })"
            />
          </div>
          <div class="w-24">
            <div class="mb-1 text-[11px] text-muted-foreground">缩放搜索 %</div>
            <Input
              :model-value="String(object!.scaleSearchPercent)"
              class="h-9"
              @update:model-value="projectStore.setObjectAdaptation(object!.id, { scaleSearchPercent: clampNumber($event, 10) })"
            />
          </div>
          <div class="max-w-xs text-[11px] text-muted-foreground">
            比例模式适合随分辨率缩放的界面；锚点模式适合贴边/居中的固定尺寸控件。搜索越大越稳健，但实时耗时越高。
          </div>
        </div>

        <!-- 视觉状态 tabs：任一启用状态命中即认为对象存在 -->
        <div class="mb-3 flex flex-wrap items-center gap-1.5">
          <button
            v-for="item in object.groups"
            :key="item.id"
            class="rounded-md border px-3 py-1.5 text-sm transition-colors cursor-pointer"
            :class="[
              item.id === projectStore.selectedGroupId
                ? 'border-primary bg-primary text-primary-foreground'
                : 'bg-background hover:bg-accent',
              !item.enabled && 'opacity-45',
            ]"
            title="双击改名"
            @click="projectStore.selectGroup(item.id)"
            @dblclick.stop="startRenameGroup(item.id, item.name)"
          >
            <input
              v-if="editingGroupId === item.id"
              v-model="editingGroupName"
              class="w-24 rounded border bg-background px-1 text-sm text-foreground"
              @click.stop
              @keydown.enter.prevent="commitRenameGroup"
              @keydown.esc="editingGroupId = null"
              @blur="commitRenameGroup"
            />
            <template v-else>{{ item.name }}</template>
          </button>
          <Button
            size="sm"
            variant="outline"
            @click="projectStore.addGroup(object!.id, '')"
          >
            <Plus class="h-3.5 w-3.5" />
            视觉状态
          </Button>
        </div>

        <!-- 点表格 -->
        <div v-if="group" class="rounded-lg border">
          <div class="flex items-center gap-2 border-b p-2.5">
            <Input
              :model-value="group.name"
              class="h-8 w-40"
              @update:model-value="
                projectStore.renameGroup(object!.id, group!.id, $event)
              "
            />
            <span class="text-xs text-muted-foreground">最少通过点数</span>
            <Input
              :model-value="String(group.minMatch)"
              class="h-8 w-16 tabular-nums"
              @update:model-value="
                projectStore.setGroupMinMatch(
                  object!.id,
                  group!.id,
                  Math.min(
                    Math.max(Number.parseInt($event, 10) || -1, -1),
                    group!.points.filter((point) => !point.mustNot).length || -1,
                  ),
                )
              "
            />
            <span class="text-xs text-muted-foreground">（-1 = 全部）</span>
            <Switch
              :checked="group.enabled"
              @update:checked="projectStore.setGroupEnabled(object!.id, group!.id, $event)"
            />
            <span class="text-xs text-muted-foreground">
              {{ group.enabled ? "参与匹配" : "已停用" }}
            </span>
            <div class="flex-1" />
            <Button size="sm" variant="outline" @click="autoGeneratePoints">
              <Wand2 class="h-3.5 w-3.5" />
              智能补点
            </Button>
            <Button
              size="sm"
              variant="outline"
              @click="projectStore.duplicateGroup(object!.id, group!.id)"
            >
              <Copy class="h-3.5 w-3.5" />
              复制状态
            </Button>
            <Button
              size="sm"
              variant="destructive"
              @click="projectStore.removeGroup(object!.id, group!.id)"
            >
              <Trash2 class="h-3.5 w-3.5" />
              删除组
            </Button>
          </div>

          <table class="w-full text-sm">
            <thead>
              <tr class="border-b text-left text-xs text-muted-foreground">
                <th class="px-2.5 py-2">#</th>
                <th class="px-2.5 py-2">dx</th>
                <th class="px-2.5 py-2">dy</th>
                <th class="px-2.5 py-2">参考色</th>
                <th class="px-2.5 py-2">容差 R/G/B</th>
                <th class="px-2.5 py-2">alpha</th>
                <th class="px-2.5 py-2">排除点</th>
                <th class="px-2.5 py-2" />
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(point, index) in group.points"
                :key="index"
                class="border-b last:border-b-0"
              >
                <td class="px-2.5 py-1.5 text-xs text-muted-foreground">
                  {{ index + 1 }}
                </td>
                <td class="px-2.5 py-1.5">
                  <Input
                    :model-value="String(point.dx)"
                    class="h-7 w-14 tabular-nums"
                    @update:model-value="
                      projectStore.updatePoint(object!.id, group!.id, index, {
                        dx: clampNumber($event, Math.max(0, object!.region.w - 1)),
                      })
                    "
                  />
                </td>
                <td class="px-2.5 py-1.5">
                  <Input
                    :model-value="String(point.dy)"
                    class="h-7 w-14 tabular-nums"
                    @update:model-value="
                      projectStore.updatePoint(object!.id, group!.id, index, {
                        dy: clampNumber($event, Math.max(0, object!.region.h - 1)),
                      })
                    "
                  />
                </td>
                <td class="px-2.5 py-1.5">
                  <div class="flex items-center gap-1.5">
                    <span
                      class="inline-block h-6 w-6 rounded border"
                      :style="{ background: rgbaToHex(point.reference) }"
                    />
                    <Input
                      :model-value="rgbaToHex(point.reference)"
                      class="h-7 w-24 font-mono text-xs"
                      @change="
                        setReference(index, ($event.target as HTMLInputElement).value)
                      "
                    />
                  </div>
                </td>
                <td class="px-2.5 py-1.5">
                  <div class="flex gap-1">
                    <Input
                      v-for="channel in [0, 1, 2]"
                      :key="channel"
                      :model-value="String(point.tolerance[channel])"
                      class="h-7 w-12 tabular-nums"
                      @update:model-value="
                        projectStore.updatePoint(object!.id, group!.id, index, {
                          tolerance: point.tolerance.map((value, i) =>
                            i === channel ? clampNumber($event, 255) : value,
                          ) as [number, number, number],
                        })
                      "
                    />
                  </div>
                </td>
                <td class="px-2.5 py-1.5">
                  <div class="flex items-center gap-1">
                    <Select
                      :model-value="point.alphaMode"
                      :options="alphaOptions"
                      class="h-7 w-28 text-xs"
                      @update:model-value="
                        projectStore.updatePoint(object!.id, group!.id, index, {
                          alphaMode: $event as 'ignore' | 'match',
                        })
                      "
                    />
                    <Input
                      v-if="point.alphaMode === 'match'"
                      :model-value="String(point.alphaTolerance)"
                      class="h-7 w-12 tabular-nums"
                      title="alpha 容差"
                      @update:model-value="
                        projectStore.updatePoint(object!.id, group!.id, index, {
                          alphaTolerance: clampNumber($event, 255),
                        })
                      "
                    />
                  </div>
                </td>
                <td class="px-2.5 py-1.5">
                  <Switch
                    :checked="point.mustNot"
                    @update:checked="
                      projectStore.updatePoint(object!.id, group!.id, index, {
                        mustNot: $event,
                      })
                    "
                  />
                </td>
                <td class="px-2.5 py-1.5 text-right">
                  <Button
                    size="icon"
                    variant="ghost"
                    class="h-7 w-7 text-muted-foreground hover:text-destructive"
                    @click="projectStore.removePoint(object!.id, group!.id, index)"
                  >
                    <Trash2 class="h-3.5 w-3.5" />
                  </Button>
                </td>
              </tr>
              <tr v-if="!group.points.length">
                <td
                  colspan="8"
                  class="px-2.5 py-6 text-center text-xs text-muted-foreground"
                >
                  还没有特征点：去捕获页用"取色加点"模式点击对象像素
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>
      <div
        v-else
        class="flex h-full items-center justify-center text-sm text-muted-foreground"
      >
        请先在左侧选择或新建一个对象
      </div>
    </div>

    <!-- 右侧：实时验证 -->
    <div class="flex w-72 shrink-0 flex-col border-l">
      <div class="space-y-2 border-b p-3">
        <div class="text-sm font-semibold">实时验证</div>
        <div class="flex gap-1.5">
          <Button
            size="sm"
            class="flex-1"
            :disabled="testing || !targetStore.bound || coordinateMismatch"
            @click="runOnce"
          >
            <Play class="h-3.5 w-3.5" />
            运行一次
          </Button>
          <Button
            size="sm"
            :variant="autoTest ? 'destructive' : 'outline'"
            :disabled="!targetStore.bound || coordinateMismatch"
            @click="toggleAutoTest"
          >
            <Square v-if="autoTest" class="h-3.5 w-3.5" />
            <Play v-else class="h-3.5 w-3.5" />
            {{ autoTest ? "停止" : "连续" }}
          </Button>
        </div>
        <div v-if="!targetStore.bound" class="text-xs text-muted-foreground">
          需要先在捕获页绑定目标窗口
        </div>

        <div v-else-if="coordinateMismatch" class="text-xs text-destructive">
          当前帧尺寸与项目基准不一致，验证已禁用
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-auto p-3">
        <template v-if="report">
          <div class="mb-3 flex items-center gap-2">
            <Badge :variant="report.matched ? 'success' : 'destructive'">
              {{ report.matched ? "对象命中" : "未命中" }}
            </Badge>
            <span class="text-xs text-muted-foreground">
              耗时 {{ (report.elapsedMicros / 1000).toFixed(2) }} ms
            </span>
            <span v-if="report.offsetX || report.offsetY || report.matchedScale !== 1" class="text-xs text-amber-500">
              偏移 {{ report.offsetX }},{{ report.offsetY }} · {{ Math.round(report.matchedScale * 100) }}%
            </span>
          </div>

          <div
            v-for="result in report.groups"
            :key="result.id"
            class="mb-2 rounded-md border p-2"
          >
            <div class="mb-1 flex items-center justify-between">
              <span class="text-xs font-medium">
                {{
                  object?.groups.find((g) => g.id === result.id)?.name ??
                  result.id
                }}
              </span>
              <Badge
                :variant="result.matched ? 'success' : 'secondary'"
                class="text-[10px]"
              >
                {{ result.passedCount }}/{{ result.required }}
              </Badge>
            </div>
            <div
              v-for="point in result.points"
              :key="point.index"
              class="mt-1.5 rounded bg-muted/50 p-1.5 text-xs"
            >
              <div class="flex items-center gap-1.5">
                <span :class="point.ok ? 'text-emerald-600' : 'text-destructive'">
                  点{{ point.index + 1 }} {{ point.ok ? "通过" : "失败" }}
                </span>
                <span class="ml-auto tabular-nums text-muted-foreground">
                  相似度 {{ point.similarity }}%
                </span>
              </div>
              <div class="mt-1 h-1.5 overflow-hidden rounded bg-muted">
                <div
                  class="h-full rounded"
                  :class="point.ok ? 'bg-emerald-500' : 'bg-destructive'"
                  :style="{ width: `${point.similarity}%` }"
                />
              </div>
              <div class="mt-1 text-muted-foreground">
                {{ point.reason || "颜色在容差内" }}；实际 {{ rgbaToHex(point.actual) }}；
                Δ {{ point.delta.join("/") }}
                <span v-if="point.maxExcess">；最大超差 +{{ point.maxExcess }}</span>
              </div>
            </div>
          </div>
        </template>
        <div v-else class="py-8 text-center text-xs text-muted-foreground">
          运行匹配后，这里显示每组形态的通过情况和失败点明细
        </div>
      </div>
    </div>
  </div>
</template>
