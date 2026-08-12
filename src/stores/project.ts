/**
 * 项目 store：特征定义数据的唯一事实来源。
 * 对象 -> 特征组 -> 采样点 三级结构，全部变更在此集中处理并标记 dirty。
 */
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  type FeatureGroup,
  type FeaturePoint,
  type ObjectSpec,
  type Project,
  type ReplayCase,
  type Region,
  CURRENT_PROJECT_VERSION,
  defaultPoint,
  emptyProject,
} from "@/lib/types";
import { shortId } from "@/lib/utils";
import {
  openProjectFile,
  loadSamplePng,
  readProjectHistory,
  saveProjectFile,
  storeSamplePngData,
} from "@/lib/ipc";

/**
 * 序列化格式：region 使用 winsitter `RectU32` 同款 LTRB 左闭右开
 * （right = left + 宽、bottom = top + 高，右/下边界不含），
 * 与脚本端区域截图裁剪口径逐像素一致。
 */
export interface ExportRegionLtrb {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export type ExportProject = Omit<Project, "objects"> & {
  objects: (Omit<ObjectSpec, "region"> & { region: ExportRegionLtrb })[];
};

/** 内部 x/y/w/h → 序列化 LTRB。 */
export function toExportProject(project: Project): ExportProject {
  return {
    ...project,
    version: CURRENT_PROJECT_VERSION,
    objects: project.objects.map((object) => ({
      ...object,
      region: {
        left: object.region.x,
        top: object.region.y,
        right: object.region.x + object.region.w,
        bottom: object.region.y + object.region.h,
      },
    })),
  };
}

/** 序列化 JSON 字符串（保存项目与导出规范共用同一口径）。 */
export function serializeProject(project: Project): string {
  const exported = toExportProject(project);
  // 保存前走一遍与导入相同的完整校验，避免生成可解析但不可执行的项目。
  parseProject(JSON.parse(JSON.stringify(exported)) as unknown);
  return JSON.stringify(exported);
}

type JsonRecord = Record<string, unknown>;

function record(value: unknown, label: string): JsonRecord {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`E4103: ${label} 必须是对象`);
  }
  return value as JsonRecord;
}

function integer(
  value: unknown,
  label: string,
  min: number,
  max = 0xffff_ffff,
): number {
  if (!Number.isSafeInteger(value) || (value as number) < min || (value as number) > max) {
    throw new Error(`E4103: ${label} 必须是 ${min}～${max} 的整数`);
  }
  return value as number;
}

function textValue(value: unknown, label: string, allowEmpty = false): string {
  if (typeof value !== "string" || (!allowEmpty && !value.trim())) {
    throw new Error(`E4103: ${label} 必须是${allowEmpty ? "" : "非空"}字符串`);
  }
  return value;
}

function tuple(
  value: unknown,
  size: number,
  label: string,
): number[] {
  if (!Array.isArray(value) || value.length !== size) {
    throw new Error(`E4103: ${label} 必须包含 ${size} 个通道`);
  }
  return value.map((channel, index) => integer(channel, `${label}[${index}]`, 0, 255));
}

function parseRegion(value: unknown, label: string): Region {
  const source = record(value, label);
  let x: number;
  let y: number;
  let w: number;
  let h: number;
  if ("left" in source || "right" in source) {
    const left = integer(source.left, `${label}.left`, 0);
    const top = integer(source.top, `${label}.top`, 0);
    const right = integer(source.right, `${label}.right`, 0);
    const bottom = integer(source.bottom, `${label}.bottom`, 0);
    if (right <= left || bottom <= top) {
      throw new Error(`E4103: ${label} 的 right/bottom 必须大于 left/top`);
    }
    x = left;
    y = top;
    w = right - left;
    h = bottom - top;
  } else {
    x = integer(source.x, `${label}.x`, 0);
    y = integer(source.y, `${label}.y`, 0);
    w = integer(source.w, `${label}.w`, 1);
    h = integer(source.h, `${label}.h`, 1);
  }
  if (x + w > 0xffff_ffff || y + h > 0xffff_ffff) {
    throw new Error(`E4103: ${label} 的右/下边界溢出`);
  }
  return { x, y, w, h };
}

/**
 * 完整反序列化与迁移：
 * - v1 接受历史 x/y/w/h，也兼容旧构建误写出的 LTRB；
 * - v2/v3 规范使用 LTRB；v4 新增坐标策略、多对象期望与外部样本；
 * - 内存中统一为 x/y/w/h，并升级到当前版本。
 */
export function parseProject(raw: unknown): Project {
  const source = record(raw, "项目");
  const version = integer(source.version, "version", 1);
  if (![1, 2, 3, CURRENT_PROJECT_VERSION].includes(version)) {
    throw new Error(`E4104: 不支持的项目版本 ${version}`);
  }
  if (!Array.isArray(source.objects)) {
    throw new Error("E4103: objects 必须是数组");
  }
  const targetSource = record(source.target ?? {}, "target");
  const target = {
    windowTitle: textValue(targetSource.windowTitle ?? "", "target.windowTitle", true),
    className: textValue(targetSource.className ?? "", "target.className", true),
    processId: integer(targetSource.processId ?? 0, "target.processId", 0),
    frameWidth: integer(targetSource.frameWidth ?? 0, "target.frameWidth", 0),
    frameHeight: integer(targetSource.frameHeight ?? 0, "target.frameHeight", 0),
    baselineDpi: integer(targetSource.baselineDpi ?? 0, "target.baselineDpi", 0, 960),
  };
  const objectIds = new Set<string>();
  const objects = source.objects.map((rawObject, objectIndex): ObjectSpec => {
    const item = record(rawObject, `objects[${objectIndex}]`);
    const id = textValue(item.id, `objects[${objectIndex}].id`);
    if (objectIds.has(id)) throw new Error(`E4103: 对象 ID 重复：${id}`);
    objectIds.add(id);
    const region = parseRegion(item.region, `objects[${objectIndex}].region`);
    if (!Array.isArray(item.groups)) {
      throw new Error(`E4103: objects[${objectIndex}].groups 必须是数组`);
    }
    const groupIds = new Set<string>();
    const groups = item.groups.map((rawGroup, groupIndex): FeatureGroup => {
      const groupSource = record(rawGroup, `objects[${objectIndex}].groups[${groupIndex}]`);
      const groupId = textValue(groupSource.id, `groups[${groupIndex}].id`);
      if (groupIds.has(groupId)) throw new Error(`E4103: 特征组 ID 重复：${groupId}`);
      groupIds.add(groupId);
      if (!Array.isArray(groupSource.points)) {
        throw new Error(`E4103: groups[${groupIndex}].points 必须是数组`);
      }
      const points = groupSource.points.map((rawPoint, pointIndex): FeaturePoint => {
        const point = record(rawPoint, `groups[${groupIndex}].points[${pointIndex}]`);
        const dx = integer(point.dx, `点 ${pointIndex + 1}.dx`, 0);
        const dy = integer(point.dy, `点 ${pointIndex + 1}.dy`, 0);
        if (dx >= region.w || dy >= region.h) {
          throw new Error(
            `E4103: 对象 ${id} 的组 ${groupId} 中点 ${pointIndex + 1} 超出区域`,
          );
        }
        const reference = tuple(point.reference, 4, `点 ${pointIndex + 1}.reference`) as [number, number, number, number];
        const tolerance = tuple(point.tolerance, 3, `点 ${pointIndex + 1}.tolerance`) as [number, number, number];
        const alphaMode = point.alphaMode ?? "ignore";
        if (alphaMode !== "ignore" && alphaMode !== "match") {
          throw new Error(`E4103: 点 ${pointIndex + 1}.alphaMode 无效`);
        }
        return {
          dx,
          dy,
          reference,
          tolerance,
          alphaMode,
          alphaTolerance: integer(point.alphaTolerance ?? 40, `点 ${pointIndex + 1}.alphaTolerance`, 0, 255),
          mustNot:
            point.mustNot == null
              ? false
              : typeof point.mustNot === "boolean"
                ? point.mustNot
                : (() => {
                    throw new Error(`E4103: 点 ${pointIndex + 1}.mustNot 必须是布尔值`);
                  })(),
        };
      });
      const minMatch = integer(groupSource.minMatch ?? -1, `组 ${groupId}.minMatch`, -1, 0x7fff_ffff);
      const regularCount = points.filter((point) => !point.mustNot).length;
      if (minMatch === 0 || minMatch > regularCount) {
        throw new Error(`E4103: 组 ${groupId}.minMatch 必须为 -1 或 1～${regularCount}`);
      }
      return {
        id: groupId,
        name: textValue(groupSource.name, `组 ${groupId}.name`, true),
        enabled:
          groupSource.enabled == null
            ? true
            : typeof groupSource.enabled === "boolean"
              ? groupSource.enabled
              : (() => {
                  throw new Error(`E4103: 组 ${groupId}.enabled 必须是布尔值`);
                })(),
        points,
        minMatch,
      };
    });
    return {
      id,
      name: textValue(item.name, `对象 ${id}.name`, true),
      region,
      groups,
      coordinateMode:
        item.coordinateMode === "scale" || item.coordinateMode === "anchor"
          ? item.coordinateMode
          : "fixed",
      anchorX:
        item.anchorX === "center" || item.anchorX === "end" ? item.anchorX : "start",
      anchorY:
        item.anchorY === "center" || item.anchorY === "end" ? item.anchorY : "start",
      searchRadius: integer(item.searchRadius ?? 0, `对象 ${id}.searchRadius`, 0, 32),
      scaleSearchPercent: integer(item.scaleSearchPercent ?? 0, `对象 ${id}.scaleSearchPercent`, 0, 10),
    };
  });
  const replaySource = source.replayCases ?? [];
  if (!Array.isArray(replaySource)) {
    throw new Error("E4103: replayCases 必须是数组");
  }
  const replayIds = new Set<string>();
  let replayEncodedBytes = 0;
  const replayCases = replaySource.map((rawCase, index): ReplayCase => {
    const item = record(rawCase, `replayCases[${index}]`);
    const id = textValue(item.id, `replayCases[${index}].id`);
    if (replayIds.has(id)) throw new Error(`E4103: 回放样本 ID 重复：${id}`);
    replayIds.add(id);
    const rawExpectations = Array.isArray(item.expectations)
      ? item.expectations
      : [{ objectId: item.objectId, expectedGroupId: item.expectedGroupId }];
    const expectationObjects = new Set<string>();
    const expectations = rawExpectations.map((rawExpectation, expectationIndex) => {
      const expectation = record(rawExpectation, `回放样本 ${id}.expectations[${expectationIndex}]`);
      const objectId = textValue(expectation.objectId, `回放样本 ${id}.objectId`);
      if (expectationObjects.has(objectId)) {
        throw new Error(`E4103: 回放样本 ${id} 重复定义对象 ${objectId}`);
      }
      expectationObjects.add(objectId);
      const object = objects.find((candidate) => candidate.id === objectId);
      if (!object) throw new Error(`E4103: 回放样本 ${id} 引用了不存在的对象 ${objectId}`);
      const expectedRaw = expectation.expectedGroupId;
      const expectedGroupId = expectedRaw == null
        ? null
        : textValue(expectedRaw, `回放样本 ${id}.expectedGroupId`);
      if (expectedGroupId && !object.groups.some((group) => group.id === expectedGroupId)) {
        throw new Error(`E4103: 回放样本 ${id} 引用了不存在的视觉状态 ${expectedGroupId}`);
      }
      return { objectId, expectedGroupId };
    });
    const storage = item.storage === "external" ? "external" : "embedded";
    const pngDataUrl = textValue(item.pngDataUrl ?? "", `回放样本 ${id}.pngDataUrl`, true);
    const relativePath = textValue(item.relativePath ?? "", `回放样本 ${id}.relativePath`, true);
    if (storage === "embedded") {
      if (!pngDataUrl.startsWith("data:image/png;base64,")) {
        throw new Error(`E4103: 回放样本 ${id} 不是内嵌 PNG`);
      }
      if (pngDataUrl.length > 24 * 1024 * 1024) {
        throw new Error(`E4103: 回放样本 ${id} 超过 24 MiB 编码上限`);
      }
      replayEncodedBytes += pngDataUrl.length;
      if (replayEncodedBytes > 64 * 1024 * 1024) {
        throw new Error("E4103: 内嵌回放样本总量超过 64 MiB 编码上限");
      }
    } else if (!relativePath || /(^|[\\/])\.\.([\\/]|$)/.test(relativePath)) {
      throw new Error(`E4103: 外部回放样本 ${id} 的相对路径无效`);
    }
    const tags = item.tags == null
      ? []
      : Array.isArray(item.tags)
        ? [...new Set(item.tags.map((tag, tagIndex) => textValue(tag, `回放样本 ${id}.tags[${tagIndex}]`)))]
        : (() => { throw new Error(`E4103: 回放样本 ${id}.tags 必须是数组`); })();
    return {
      id,
      name: textValue(item.name, `回放样本 ${id}.name`, true),
      capturedAt: integer(item.capturedAt, `回放样本 ${id}.capturedAt`, 0, Number.MAX_SAFE_INTEGER),
      width: integer(item.width, `回放样本 ${id}.width`, 1, 32768),
      height: integer(item.height, `回放样本 ${id}.height`, 1, 32768),
      storage,
      pngDataUrl,
      relativePath,
      sha256: textValue(item.sha256 ?? "", `回放样本 ${id}.sha256`, true),
      expectations,
      tags,
    };
  });
  return { version: CURRENT_PROJECT_VERSION, target, objects, replayCases };
}

const RECOVERY_KEY = "imagesitter.project.recovery.v4";
const LEGACY_RECOVERY_KEYS = ["imagesitter.project.recovery.v3", "imagesitter.project.recovery.v2"];
const TEMPLATE_KEY = "imagesitter.object.templates.v1";

export interface ObjectTemplate {
  id: string;
  name: string;
  savedAt: number;
  object: ObjectSpec;
}

function loadTemplates(): ObjectTemplate[] {
  try {
    const raw = localStorage.getItem(TEMPLATE_KEY);
    if (!raw) return [];
    const items = JSON.parse(raw) as unknown;
    if (!Array.isArray(items)) return [];
    return items
      .filter((item): item is ObjectTemplate => {
        if (!item || typeof item !== "object") return false;
        const value = item as Partial<ObjectTemplate>;
        return typeof value.id === "string" && typeof value.name === "string" &&
          typeof value.savedAt === "number" && !!value.object && Array.isArray(value.object.groups);
      })
      .slice(0, 50);
  } catch {
    return [];
  }
}

export const useProjectStore = defineStore("project", () => {
  const project = ref<Project>(emptyProject());
  const filePath = ref<string | null>(null);
  const dirty = ref(false);
  const templates = ref<ObjectTemplate[]>(loadTemplates());
  /** localStorage 容量不足时，自动恢复副本会省略内嵌 PNG。 */
  const replayRecoveryOmitted = ref(false);
  let recoveryTimer: ReturnType<typeof setTimeout> | null = null;
  const undoStack = ref<string[]>([]);
  const redoStack = ref<string[]>([]);
  let lastSnapshot = JSON.stringify(project.value);

  const selectedObjectId = ref<string | null>(null);
  const selectedGroupId = ref<string | null>(null);

  const selectedObject = computed<ObjectSpec | null>(() =>
    project.value.objects.find((o) => o.id === selectedObjectId.value) ?? null,
  );

  const selectedGroup = computed<FeatureGroup | null>(() =>
    selectedObject.value?.groups.find((g) => g.id === selectedGroupId.value) ??
    null,
  );
  const canUndo = computed(() => undoStack.value.length > 0);
  const canRedo = computed(() => redoStack.value.length > 0);

  function touch() {
    const currentSnapshot = JSON.stringify(project.value);
    if (currentSnapshot !== lastSnapshot) {
      undoStack.value.push(lastSnapshot);
      // 内嵌回放帧会放大快照；同时限制条数和约 24 MiB 历史体积。
      let historyBytes = undoStack.value.reduce((sum, item) => sum + item.length, 0);
      while (undoStack.value.length > 100 || historyBytes > 24 * 1024 * 1024) {
        historyBytes -= undoStack.value.shift()?.length ?? 0;
      }
      redoStack.value = [];
      lastSnapshot = currentSnapshot;
    }
    dirty.value = true;
    if (recoveryTimer) clearTimeout(recoveryTimer);
    recoveryTimer = setTimeout(flushRecovery, 250);
  }

  function restoreSnapshot(snapshot: string) {
    const objectId = selectedObjectId.value;
    const groupId = selectedGroupId.value;
    project.value = parseProject(JSON.parse(snapshot) as unknown);
    lastSnapshot = JSON.stringify(project.value);
    const object =
      project.value.objects.find((item) => item.id === objectId) ??
      project.value.objects[0];
    selectedObjectId.value = object?.id ?? null;
    selectedGroupId.value =
      object?.groups.find((item) => item.id === groupId)?.id ??
      object?.groups[0]?.id ??
      null;
    dirty.value = true;
    flushRecovery();
  }

  function undo() {
    const snapshot = undoStack.value.pop();
    if (!snapshot) return;
    redoStack.value.push(JSON.stringify(project.value));
    restoreSnapshot(snapshot);
  }

  function redo() {
    const snapshot = redoStack.value.pop();
    if (!snapshot) return;
    undoStack.value.push(JSON.stringify(project.value));
    restoreSnapshot(snapshot);
  }

  function resetHistory() {
    undoStack.value = [];
    redoStack.value = [];
    lastSnapshot = JSON.stringify(project.value);
  }

  function persistTemplates() {
    try {
      localStorage.setItem(TEMPLATE_KEY, JSON.stringify(templates.value.slice(0, 50)));
    } catch {
      throw new Error("E4501: 无法保存对象模板，本地存储空间不足");
    }
  }

  function saveObjectTemplate(objectId: string, name: string): ObjectTemplate | null {
    const object = project.value.objects.find((item) => item.id === objectId);
    if (!object) return null;
    const template: ObjectTemplate = {
      id: shortId("tpl"),
      name: name.trim() || object.name,
      savedAt: Date.now(),
      object: structuredClone(object),
    };
    templates.value.unshift(template);
    templates.value = templates.value.slice(0, 50);
    persistTemplates();
    return template;
  }

  function addObjectFromTemplate(templateId: string): ObjectSpec | null {
    const template = templates.value.find((item) => item.id === templateId);
    if (!template) return null;
    const object: ObjectSpec = {
      ...structuredClone(template.object),
      id: shortId("obj"),
      name: template.name,
      groups: template.object.groups.map((group) => ({
        ...structuredClone(group),
        id: shortId("grp"),
      })),
    };
    project.value.objects.push(object);
    selectObject(object.id);
    touch();
    return object;
  }

  function removeObjectTemplate(templateId: string) {
    const before = templates.value.length;
    templates.value = templates.value.filter((item) => item.id !== templateId);
    if (templates.value.length !== before) persistTemplates();
  }

  function flushRecovery() {
    if (recoveryTimer) {
      clearTimeout(recoveryTimer);
      recoveryTimer = null;
    }
    if (!dirty.value) return;
    try {
      const recoveryProject = toExportProject(project.value);
      let replayCasesOmitted = false;
      // 浏览器 localStorage 常见配额约 5 MiB；优先保证规则可恢复。
      if (JSON.stringify(recoveryProject).length > 3_500_000 && recoveryProject.replayCases.length) {
        recoveryProject.replayCases = [];
        replayCasesOmitted = true;
      }
      localStorage.setItem(
        RECOVERY_KEY,
        JSON.stringify({
          savedAt: Date.now(),
          filePath: filePath.value,
          project: recoveryProject,
          replayCasesOmitted,
        }),
      );
      replayRecoveryOmitted.value = replayCasesOmitted;
    } catch {
      // localStorage 不可用或空间不足时不影响主工作流。
      replayRecoveryOmitted.value = project.value.replayCases.length > 0;
    }
  }

  function clearRecovery() {
    if (recoveryTimer) clearTimeout(recoveryTimer);
    recoveryTimer = null;
    replayRecoveryOmitted.value = false;
    try {
      localStorage.removeItem(RECOVERY_KEY);
      for (const key of LEGACY_RECOVERY_KEYS) localStorage.removeItem(key);
    } catch {
      // 浏览器预览禁用存储时忽略。
    }
  }

  function restoreRecovery(): number | null {
    try {
      const raw = localStorage.getItem(RECOVERY_KEY) ??
        LEGACY_RECOVERY_KEYS.map((key) => localStorage.getItem(key)).find(Boolean);
      if (!raw) return null;
      const payload = record(JSON.parse(raw) as unknown, "恢复数据");
      project.value = parseProject(payload.project);
      replayRecoveryOmitted.value = payload.replayCasesOmitted === true;
      filePath.value = typeof payload.filePath === "string" ? payload.filePath : null;
      dirty.value = true;
      selectObject(project.value.objects[0]?.id ?? null);
      resetHistory();
      return typeof payload.savedAt === "number" ? payload.savedAt : Date.now();
    } catch {
      clearRecovery();
      return null;
    }
  }

  function selectObject(id: string | null) {
    selectedObjectId.value = id;
    const object = project.value.objects.find((o) => o.id === id);
    selectedGroupId.value = object?.groups[0]?.id ?? null;
  }

  function selectGroup(id: string | null) {
    selectedGroupId.value = id;
  }

  function addObject(name: string): ObjectSpec {
    const object: ObjectSpec = {
      id: shortId("obj"),
      name,
      region: { x: 0, y: 0, w: 64, h: 64 },
      groups: [createGroup("形态一")],
      coordinateMode: "fixed",
      anchorX: "start",
      anchorY: "start",
      searchRadius: 0,
      scaleSearchPercent: 0,
    };
    project.value.objects.push(object);
    selectObject(object.id);
    touch();
    return object;
  }

  function createGroup(name: string): FeatureGroup {
    return { id: shortId("grp"), name, enabled: true, points: [], minMatch: -1 };
  }

  function addGroup(objectId: string, name: string) {
    const object = project.value.objects.find((o) => o.id === objectId);
    if (!object) return;
    const group = createGroup(name || `形态${object.groups.length + 1}`);
    object.groups.push(group);
    selectedGroupId.value = group.id;
    touch();
  }

  function removeGroup(objectId: string, groupId: string) {
    const object = project.value.objects.find((o) => o.id === objectId);
    if (!object) return;
    object.groups = object.groups.filter((g) => g.id !== groupId);
    for (const item of project.value.replayCases) {
      for (const expectation of item.expectations) {
        if (expectation.objectId === objectId && expectation.expectedGroupId === groupId) {
          expectation.expectedGroupId = null;
        }
      }
    }
    if (selectedGroupId.value === groupId) {
      selectedGroupId.value = object.groups[0]?.id ?? null;
    }
    touch();
  }

  function renameGroup(objectId: string, groupId: string, name: string) {
    const group = project.value.objects
      .find((o) => o.id === objectId)
      ?.groups.find((g) => g.id === groupId);
    if (group) {
      group.name = name;
      touch();
    }
  }

  function setGroupMinMatch(objectId: string, groupId: string, minMatch: number) {
    const group = project.value.objects
      .find((o) => o.id === objectId)
      ?.groups.find((g) => g.id === groupId);
    if (group) {
      const regularCount = group.points.filter((point) => !point.mustNot).length;
      group.minMatch =
        minMatch < 0 || regularCount === 0
          ? -1
          : Math.min(Math.max(Math.trunc(minMatch), 1), regularCount);
      touch();
    }
  }

  function removeObject(objectId: string) {
    project.value.objects = project.value.objects.filter(
      (o) => o.id !== objectId,
    );
    project.value.replayCases = project.value.replayCases
      .map((item) => ({
        ...item,
        expectations: item.expectations.filter((expectation) => expectation.objectId !== objectId),
      }))
      .filter((item) => item.expectations.length > 0);
    if (selectedObjectId.value === objectId) {
      selectObject(project.value.objects[0]?.id ?? null);
    }
    touch();
  }

  function renameObject(objectId: string, name: string) {
    const object = project.value.objects.find((o) => o.id === objectId);
    if (object) {
      object.name = name;
      touch();
    }
  }

  function setRegion(objectId: string, region: Region) {
    const object = project.value.objects.find((o) => o.id === objectId);
    const valid =
      Number.isSafeInteger(region.x) &&
      Number.isSafeInteger(region.y) &&
      Number.isSafeInteger(region.w) &&
      Number.isSafeInteger(region.h) &&
      region.x >= 0 &&
      region.y >= 0 &&
      region.w > 0 &&
      region.h > 0 &&
      region.x + region.w <= 0xffff_ffff &&
      region.y + region.h <= 0xffff_ffff &&
      object?.groups.every((group) =>
        group.points.every((point) => point.dx < region.w && point.dy < region.h),
      );
    if (object && valid) {
      object.region = { ...region };
      touch();
      return true;
    }
    return false;
  }

  function setTargetHint(windowTitle: string, className: string, processId: number) {
    const current = project.value.target;
    if (
      current.windowTitle === windowTitle &&
      current.className === className &&
      current.processId === processId
    ) return;
    const sameWindowKind =
      current.windowTitle === windowTitle && current.className === className;
    project.value.target = {
      windowTitle,
      className,
      processId,
      frameWidth: sameWindowKind ? current.frameWidth : 0,
      frameHeight: sameWindowKind ? current.frameHeight : 0,
      baselineDpi: sameWindowKind ? current.baselineDpi : 0,
    };
    touch();
  }

  function setGroupEnabled(objectId: string, groupId: string, enabled: boolean) {
    const group = project.value.objects
      .find((o) => o.id === objectId)
      ?.groups.find((g) => g.id === groupId);
    if (group && group.enabled !== enabled) {
      group.enabled = enabled;
      touch();
    }
  }

  function duplicateGroup(objectId: string, groupId: string) {
    const object = project.value.objects.find((item) => item.id === objectId);
    const source = object?.groups.find((item) => item.id === groupId);
    if (!object || !source) return;
    const copy: FeatureGroup = {
      ...structuredClone(source),
      id: shortId("grp"),
      name: `${source.name} 副本`,
    };
    object.groups.push(copy);
    selectedGroupId.value = copy.id;
    touch();
  }

  function setTargetFrameSize(frameWidth: number, frameHeight: number, baselineDpi = 0) {
    if (!Number.isSafeInteger(frameWidth) || !Number.isSafeInteger(frameHeight)) return;
    if (frameWidth <= 0 || frameHeight <= 0) return;
    if (
      project.value.target.frameWidth === frameWidth &&
      project.value.target.frameHeight === frameHeight
    ) return;
    project.value.target.frameWidth = frameWidth;
    project.value.target.frameHeight = frameHeight;
    if (baselineDpi > 0) project.value.target.baselineDpi = baselineDpi;
    touch();
  }

  function setObjectAdaptation(
    objectId: string,
    patch: Partial<Pick<ObjectSpec, "coordinateMode" | "anchorX" | "anchorY" | "searchRadius" | "scaleSearchPercent">>,
  ) {
    const object = project.value.objects.find((item) => item.id === objectId);
    if (!object) return;
    if (patch.coordinateMode) object.coordinateMode = patch.coordinateMode;
    if (patch.anchorX) object.anchorX = patch.anchorX;
    if (patch.anchorY) object.anchorY = patch.anchorY;
    if (patch.searchRadius != null) object.searchRadius = Math.min(32, Math.max(0, Math.trunc(patch.searchRadius)));
    if (patch.scaleSearchPercent != null) {
      object.scaleSearchPercent = Math.min(10, Math.max(0, Math.trunc(patch.scaleSearchPercent)));
    }
    touch();
  }

  function addPoint(
    objectId: string,
    groupId: string,
    dx: number,
    dy: number,
    reference: [number, number, number, number],
  ): FeaturePoint | null {
    const object = project.value.objects.find((item) => item.id === objectId);
    const group = object?.groups.find((g) => g.id === groupId);
    if (!object || !group || dx < 0 || dy < 0 || dx >= object.region.w || dy >= object.region.h) {
      return null;
    }
    const point = defaultPoint(dx, dy, reference);
    group.points.push(point);
    touch();
    return point;
  }

  /** 批量添加自动候选点，只生成一次撤销快照。 */
  function addPointsBatch(
    objectId: string,
    groupId: string,
    candidates: {
      dx: number;
      dy: number;
      reference: [number, number, number, number];
      tolerance?: [number, number, number];
    }[],
  ): number {
    const object = project.value.objects.find((item) => item.id === objectId);
    const group = object?.groups.find((item) => item.id === groupId);
    if (!object || !group) return 0;
    const occupied = new Set(group.points.map((point) => `${point.dx}:${point.dy}`));
    let added = 0;
    for (const candidate of candidates) {
      const key = `${candidate.dx}:${candidate.dy}`;
      if (
        occupied.has(key) ||
        candidate.dx < 0 ||
        candidate.dy < 0 ||
        candidate.dx >= object.region.w ||
        candidate.dy >= object.region.h
      ) continue;
      const point = defaultPoint(candidate.dx, candidate.dy, candidate.reference);
      if (candidate.tolerance) point.tolerance = [...candidate.tolerance];
      group.points.push(point);
      occupied.add(key);
      added += 1;
    }
    if (added > 0) touch();
    return added;
  }

  function updatePoint(
    objectId: string,
    groupId: string,
    index: number,
    patch: Partial<FeaturePoint>,
  ) {
    const object = project.value.objects.find((item) => item.id === objectId);
    const group = object?.groups.find((g) => g.id === groupId);
    const point = group?.points[index];
    const nextDx = patch.dx ?? point?.dx ?? 0;
    const nextDy = patch.dy ?? point?.dy ?? 0;
    if (
      object &&
      group &&
      point &&
      nextDx >= 0 &&
      nextDy >= 0 &&
      nextDx < object.region.w &&
      nextDy < object.region.h
    ) {
      Object.assign(point, patch);
      const regularCount = group.points.filter((item) => !item.mustNot).length;
      if (group.minMatch > regularCount) {
        group.minMatch = regularCount > 0 ? regularCount : -1;
      }
      touch();
    }
  }

  function removePoint(objectId: string, groupId: string, index: number) {
    const group = project.value.objects
      .find((o) => o.id === objectId)
      ?.groups.find((g) => g.id === groupId);
    if (group && index >= 0 && index < group.points.length) {
      group.points.splice(index, 1);
      const regularCount = group.points.filter((point) => !point.mustNot).length;
      if (group.minMatch > regularCount) {
        group.minMatch = regularCount > 0 ? regularCount : -1;
      }
      touch();
    }
  }

  /** 应用校准建议到指定组 */
  function applySuggestions(
    objectId: string,
    groupId: string,
    suggestions: {
      index: number;
      reference: [number, number, number, number];
      tolerance: [number, number, number];
      suggestedAlphaMode: "ignore" | "match";
      alphaTolerance: number;
    }[],
  ) {
    const group = project.value.objects
      .find((o) => o.id === objectId)
      ?.groups.find((g) => g.id === groupId);
    if (!group) return;
    for (const suggestion of suggestions) {
      const point = group.points[suggestion.index];
      if (!point) continue;
      point.reference = [...suggestion.reference] as typeof point.reference;
      point.tolerance = [...suggestion.tolerance] as typeof point.tolerance;
      point.alphaMode = suggestion.suggestedAlphaMode;
      point.alphaTolerance = suggestion.alphaTolerance;
    }
    touch();
  }

  /** 删除校准中被判定为低质量的点，返回删除数量。 */
  function removeRejectedPoints(
    objectId: string,
    groupId: string,
    suggestions: { index: number; recommendKeep: boolean }[],
  ) {
    const group = project.value.objects
      .find((o) => o.id === objectId)
      ?.groups.find((g) => g.id === groupId);
    if (!group) return 0;
    const rejected = new Set(
      suggestions.filter((item) => !item.recommendKeep).map((item) => item.index),
    );
    const before = group.points.length;
    group.points = group.points.filter((_, index) => !rejected.has(index));
    const removed = before - group.points.length;
    if (removed > 0) {
      const regularCount = group.points.filter((point) => !point.mustNot).length;
      if (group.minMatch > regularCount) group.minMatch = regularCount || -1;
      touch();
    }
    return removed;
  }

  function addReplayCase(input: Omit<ReplayCase, "id" | "capturedAt">): ReplayCase {
    return addReplayCasesBatch([input])[0];
  }

  function addReplayCasesBatch(
    inputs: Omit<ReplayCase, "id" | "capturedAt">[],
  ): ReplayCase[] {
    const startedAt = Date.now();
    const replayCases = inputs.map((input, index): ReplayCase => ({
      ...input,
      id: shortId("case"),
      capturedAt: startedAt + index,
    }));
    const total = project.value.replayCases.reduce(
      (sum, item) => sum + item.pngDataUrl.length,
      replayCases.reduce((sum, item) => sum + item.pngDataUrl.length, 0),
    );
    if (replayCases.some((item) => item.pngDataUrl.length > 24 * 1024 * 1024) || total > 64 * 1024 * 1024) {
      throw new Error("E4401: 回放样本容量超过项目安全上限");
    }
    project.value.replayCases.push(...replayCases);
    if (replayCases.length) touch();
    return replayCases;
  }

  function renameReplayCase(id: string, name: string) {
    const item = project.value.replayCases.find((candidate) => candidate.id === id);
    if (item && item.name !== name) {
      item.name = name;
      touch();
    }
  }

  function setReplayExpectations(id: string, expectations: ReplayCase["expectations"]) {
    const item = project.value.replayCases.find((candidate) => candidate.id === id);
    if (!item) return;
    const objectIds = new Set<string>();
    item.expectations = expectations.filter((expectation) => {
      if (objectIds.has(expectation.objectId)) return false;
      const object = project.value.objects.find((candidate) => candidate.id === expectation.objectId);
      if (!object) return false;
      if (
        expectation.expectedGroupId &&
        !object.groups.some((group) => group.id === expectation.expectedGroupId)
      ) return false;
      objectIds.add(expectation.objectId);
      return true;
    });
    touch();
  }

  function setReplayTags(id: string, tags: string[]) {
    const item = project.value.replayCases.find((candidate) => candidate.id === id);
    if (!item) return;
    item.tags = [...new Set(tags.map((tag) => tag.trim()).filter(Boolean))].slice(0, 20);
    touch();
  }

  function removeReplayCase(id: string) {
    const before = project.value.replayCases.length;
    project.value.replayCases = project.value.replayCases.filter((item) => item.id !== id);
    if (project.value.replayCases.length !== before) touch();
  }

  function removeReplayCases(ids: string[]) {
    const selected = new Set(ids);
    const before = project.value.replayCases.length;
    project.value.replayCases = project.value.replayCases.filter((item) => !selected.has(item.id));
    if (project.value.replayCases.length !== before) touch();
  }

  async function saveTo(path: string) {
    let saving = project.value;
    // 另存为新路径时同步迁移受管外置样本，避免新项目引用旧目录。
    if (filePath.value && filePath.value !== path && project.value.replayCases.some((item) => item.storage === "external")) {
      saving = structuredClone(project.value);
      for (const sample of saving.replayCases) {
        if (sample.storage !== "external") continue;
        const loaded = await loadSamplePng(filePath.value, sample.relativePath, sample.sha256);
        const migrated = await storeSamplePngData(path, loaded.pngDataUrl, "external");
        sample.relativePath = migrated.relativePath;
        sample.sha256 = migrated.sha256;
      }
    }
    await saveProjectFile(path, serializeProject(saving));
    project.value = saving;
    filePath.value = path;
    dirty.value = false;
    resetHistory();
    clearRecovery();
  }

  async function loadFrom(path: string) {
    const loaded = await openProjectFile(path);
    project.value = parseProject(loaded);
    filePath.value = path;
    dirty.value = false;
    clearRecovery();
    selectObject(project.value.objects[0]?.id ?? null);
    resetHistory();
  }

  /** 恢复为待保存状态，不会静默覆盖主文件。 */
  async function restoreHistory(fileName: string) {
    if (!filePath.value) throw new Error("E4450: 请先保存或打开项目");
    const restored = parseProject(await readProjectHistory(filePath.value, fileName));
    undoStack.value.push(JSON.stringify(project.value));
    redoStack.value = [];
    project.value = restored;
    selectObject(project.value.objects[0]?.id ?? null);
    lastSnapshot = JSON.stringify(project.value);
    dirty.value = true;
    flushRecovery();
  }

  /** 合并另一项目的规则并重新分配 ID；回放帧不合并，避免外部路径失效。 */
  function mergeProject(incoming: Project): { objects: number; groups: number; points: number } {
    const currentTarget = project.value.target;
    const incomingTarget = incoming.target;
    const currentHasRules = project.value.objects.length > 0;
    if (
      currentHasRules &&
      currentTarget.frameWidth > 0 &&
      incomingTarget.frameWidth > 0 &&
      (currentTarget.frameWidth !== incomingTarget.frameWidth ||
        currentTarget.frameHeight !== incomingTarget.frameHeight)
    ) {
      throw new Error(
        `E4510: 项目基准尺寸不一致（${currentTarget.frameWidth}×${currentTarget.frameHeight} / ${incomingTarget.frameWidth}×${incomingTarget.frameHeight}）`,
      );
    }
    if (!currentHasRules && incomingTarget.frameWidth > 0) {
      project.value.target = structuredClone(incomingTarget);
    }
    let groups = 0;
    let points = 0;
    const existingNames = new Set(project.value.objects.map((item) => item.name));
    for (const source of incoming.objects) {
      let name = source.name;
      let suffix = 2;
      while (existingNames.has(name)) name = `${source.name} (${suffix++})`;
      existingNames.add(name);
      const object: ObjectSpec = {
        ...structuredClone(source),
        id: shortId("obj"),
        name,
        groups: source.groups.map((group) => {
          groups += 1;
          points += group.points.length;
          return { ...structuredClone(group), id: shortId("grp") };
        }),
      };
      project.value.objects.push(object);
    }
    if (incoming.objects.length > 0) {
      selectObject(project.value.objects.at(-incoming.objects.length)?.id ?? null);
      touch();
    }
    return { objects: incoming.objects.length, groups, points };
  }

  function reset() {
    project.value = emptyProject();
    filePath.value = null;
    dirty.value = false;
    clearRecovery();
    selectedObjectId.value = null;
    selectedGroupId.value = null;
    resetHistory();
  }

  return {
    project,
    filePath,
    dirty,
    templates,
    replayRecoveryOmitted,
    selectedObjectId,
    selectedGroupId,
    selectedObject,
    selectedGroup,
    canUndo,
    canRedo,
    selectObject,
    selectGroup,
    addObject,
    addGroup,
    removeGroup,
    renameGroup,
    setGroupEnabled,
    duplicateGroup,
    setGroupMinMatch,
    removeObject,
    renameObject,
    setRegion,
    setTargetHint,
    setTargetFrameSize,
    setObjectAdaptation,
    addPoint,
    addPointsBatch,
    updatePoint,
    removePoint,
    applySuggestions,
    removeRejectedPoints,
    addReplayCase,
    addReplayCasesBatch,
    renameReplayCase,
    setReplayExpectations,
    setReplayTags,
    removeReplayCase,
    removeReplayCases,
    saveTo,
    loadFrom,
    reset,
    flushRecovery,
    restoreRecovery,
    undo,
    redo,
    saveObjectTemplate,
    addObjectFromTemplate,
    removeObjectTemplate,
    restoreHistory,
    mergeProject,
  };
});
