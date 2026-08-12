<script setup lang="ts">
/**
 * 导出页：查看导出规范、保存项目文件、复制集成说明。
 */
import { save } from "@tauri-apps/plugin-dialog";
import { Archive, Copy, FileDown, Save } from "lucide-vue-next";
import { computed, ref } from "vue";
import { toast } from "vue-sonner";
import Button from "@/components/ui/button/Button.vue";
import Card from "@/components/ui/card/Card.vue";
import CardContent from "@/components/ui/card/CardContent.vue";
import CardHeader from "@/components/ui/card/CardHeader.vue";
import CardTitle from "@/components/ui/card/CardTitle.vue";
import TabsContentView from "@/components/ui/tabs/TabsContentView.vue";
import TabsListView from "@/components/ui/tabs/TabsListView.vue";
import TabsRoot from "@/components/ui/tabs/TabsRoot.vue";
import TabsTriggerView from "@/components/ui/tabs/TabsTriggerView.vue";
import { loadSamplePng, saveProjectFile } from "@/lib/ipc";
import { serializeProject, useProjectStore } from "@/stores/project";

const projectStore = useProjectStore();
const tab = ref("spec");
const packaging = ref(false);

/**
 * 导出规范 JSON：与保存项目文件同一口径，region 为 winsitter `RectU32`
 * 同款 LTRB 左闭右开（right = left + 宽、bottom = top + 高，右/下边界不含）。
 */
const specJson = computed(() =>
  JSON.stringify(
    {
      ...JSON.parse(serializeProject(projectStore.project)),
      // 回放 PNG 仅用于作者工具，不复制到运行时规范，避免大 Base64 阻塞预览/剪贴板。
      replayCases: [],
    },
    null,
    2,
  ),
);

const pythonSnippet = `import json
from PIL import ImageGrab  # 或你自己的截图/辅助框架

def load_spec(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)

def match_groups(frame, region, groups):
    """frame: 带 .getpixel((x, y)) 的图像对象；返回命中的组名列表

    region 为 LTRB 左闭右开（同 winsitter RectU32）：
    按 [left:right] x [top:bottom] 裁剪客户区截图即得选区图像，
    组内点的 dx/dy 相对选区左上角，可直接在裁剪图上取色。
    """
    if getattr(frame, "mode", None) != "RGBA":
        frame = frame.convert("RGBA")
    hits = []
    for group in groups:
        if not group.get("enabled", True):
            continue
        passed = 0
        required_total = 0
        exclusions_ok = True
        for p in group["points"]:
            x, y = region["left"] + p["dx"], region["top"] + p["dy"]
            actual = frame.getpixel((x, y))  # (r, g, b) 或 (r, g, b, a)
            ref, tol = p["reference"], p["tolerance"]
            matched = all(
                abs(actual[c] - ref[c]) <= tol[c] for c in range(3)
            )
            if p.get("alphaMode") == "match":
                matched = matched and abs(actual[3] - ref[3]) <= p["alphaTolerance"]
            if p.get("mustNot"):
                exclusions_ok &= not matched
            else:
                required_total += 1
                passed += matched
        need = group.get("minMatch", -1)
        need = required_total if need < 0 else min(need, required_total)
        if exclusions_ok and required_total > 0 and passed >= need:
            hits.append(group["name"])
    return hits

spec = load_spec("imagesitter-project.json")
for obj in spec["objects"]:
    # frame = 目标窗口客户区截图（已去掉窗口边框，与本工具预览一致）
    # shot = frame.crop((obj["region"]["left"], obj["region"]["top"],
    #                    obj["region"]["right"], obj["region"]["bottom"]))
    # hit = match_groups(frame, obj["region"], obj["groups"])
    # hit 非空即对象出现（任意一种形态命中）
    pass`;

async function copySpec() {
  await navigator.clipboard.writeText(specJson.value);
  toast.success("规范 JSON 已复制到剪贴板");
}

async function copyPython() {
  await navigator.clipboard.writeText(pythonSnippet);
  toast.success("Python 示例已复制到剪贴板");
}

async function saveProject() {
  const path = await save({
    title: "保存 ImageSitter 项目",
    defaultPath: projectStore.filePath ?? "imagesitter-project.json",
    filters: [{ name: "ImageSitter 项目", extensions: ["json"] }],
  });
  if (!path) return;
  try {
    await projectStore.saveTo(path);
    toast.success("项目已保存");
  } catch (error) {
    toast.error(String(error));
  }
}

/** 把外置样本嵌入单一 JSON，用于跨机器交付和归档。 */
async function exportPortableProject() {
  const external = projectStore.project.replayCases.filter((item) => item.storage === "external");
  if (external.length && !projectStore.filePath) {
    toast.error("外置样本需要已保存项目才能定位");
    return;
  }
  const path = await save({
    title: "导出便携 ImageSitter 项目",
    defaultPath: "imagesitter-portable.json",
    filters: [{ name: "ImageSitter 便携项目", extensions: ["json"] }],
  });
  if (!path) return;
  packaging.value = true;
  try {
    const portable = structuredClone(projectStore.project);
    for (const sample of portable.replayCases) {
      if (sample.storage !== "external") continue;
      const loaded = await loadSamplePng(
        projectStore.filePath!,
        sample.relativePath,
        sample.sha256,
      );
      sample.storage = "embedded";
      sample.pngDataUrl = loaded.pngDataUrl;
      sample.relativePath = "";
      sample.sha256 = loaded.sha256;
    }
    await saveProjectFile(path, serializeProject(portable));
    toast.success(`便携项目已导出，共嵌入 ${external.length} 个外置样本`);
  } catch (error) {
    toast.error(`${String(error)}。若内嵌样本超过 64 MiB，请保留项目与 .samples 目录一起交付。`);
  } finally {
    packaging.value = false;
  }
}
</script>

<template>
  <div class="mx-auto max-w-4xl p-6">
    <div class="mb-4 flex items-center gap-2">
      <h1 class="text-lg font-semibold">导出与集成</h1>
      <div class="flex-1" />
      <Button variant="outline" :disabled="packaging" @click="exportPortableProject">
        <Archive class="h-4 w-4" />
        {{ packaging ? "打包中…" : "导出便携项目" }}
      </Button>
      <Button @click="saveProject">
        <Save class="h-4 w-4" />
        保存项目
      </Button>
    </div>

    <TabsRoot v-model="tab">
      <TabsListView>
        <TabsTriggerView value="spec">规范 JSON</TabsTriggerView>
        <TabsTriggerView value="python">Python 集成</TabsTriggerView>
        <TabsTriggerView value="semantics">匹配语义</TabsTriggerView>
      </TabsListView>

      <TabsContentView value="spec">
        <Card>
          <CardHeader class="flex-row items-center justify-between space-y-0">
            <CardTitle class="text-sm">运行时规范 JSON（不含回放帧）</CardTitle>
            <Button size="sm" variant="outline" @click="copySpec">
              <Copy class="h-3.5 w-3.5" />
              复制
            </Button>
          </CardHeader>
          <CardContent>
            <pre
              class="max-h-[28rem] overflow-auto rounded-md bg-muted p-3 text-xs leading-relaxed"
            >{{ specJson }}</pre>
          </CardContent>
        </Card>
      </TabsContentView>

      <TabsContentView value="python">
        <Card>
          <CardHeader class="flex-row items-center justify-between space-y-0">
            <CardTitle class="text-sm">辅助脚本加载器示例（Python）</CardTitle>
            <Button size="sm" variant="outline" @click="copyPython">
              <Copy class="h-3.5 w-3.5" />
              复制
            </Button>
          </CardHeader>
          <CardContent>
            <pre
              class="max-h-[28rem] overflow-auto rounded-md bg-muted p-3 text-xs leading-relaxed"
            >{{ pythonSnippet }}</pre>
          </CardContent>
        </Card>
      </TabsContentView>

      <TabsContentView value="semantics">
        <Card>
          <CardHeader>
            <CardTitle class="text-sm">匹配语义约定</CardTitle>
          </CardHeader>
          <CardContent class="space-y-2 text-sm text-muted-foreground">
            <p>
              · 坐标系原点是目标窗口客户区左上角（捕获帧已裁掉窗口边框）；
              region 导出为 LTRB 左闭右开（同 winsitter RectU32），
              right = left + 宽、bottom = top + 高，右/下边界不含。
            </p>
            <p>
              · 点的 dx/dy 相对选区左上角；在客户区截图上按
              [left:right] × [top:bottom] 裁剪即得选区图像，
              两者像素逐一对应，无 ±1 偏差。
            </p>
            <p>
              · 常规点：RGB 各通道 |实际 - 参考| ≤ tolerance 即通过；
              alphaMode 为 ignore 时不比 alpha，为 match 时按 alphaTolerance 比对。
            </p>
            <p>
              · 排除点（mustNot）：与参考色"不匹配"才通过，用于排除相似对象。
            </p>
            <p>
              · 组命中：通过的常规点数 ≥ minMatch（-1 表示全部）且排除点全部通过。
            </p>
            <p>· enabled=false 的视觉状态不会参与匹配。</p>
            <p>· 对象命中：任意一个形态组命中即视为对象出现。</p>
            <p>
              · 逐点采样复杂度 O(点数)，无图像卷积，适合游戏场景的高频实时判断。
            </p>
          </CardContent>
        </Card>
      </TabsContentView>
    </TabsRoot>

    <div
      class="mt-4 flex items-center gap-2 text-xs text-muted-foreground"
    >
      <FileDown class="h-3.5 w-3.5" />
      “保存项目”会保留内嵌回放帧；复制的运行时规范会清空 replayCases，避免把测试图像带入辅助脚本。
      region 为 LTRB 左闭右开（与 winsitter 一致）。
    </div>
  </div>
</template>
