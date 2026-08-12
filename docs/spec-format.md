# ImageSitter 项目与导出规范（v4）

项目文件是 UTF-8 JSON。区域使用客户区坐标下的 LTRB 左闭右开形式；内存和 IPC 使用 `x/y/w/h`，保存时转换为 LTRB。

```jsonc
{
  "version": 4,
  "target": {
    "windowTitle": "游戏窗口标题",
    "className": "",
    "processId": 0,
    "frameWidth": 1920,
    "frameHeight": 1080,
    "baselineDpi": 96
  },
  "objects": [{
    "id": "obj_xxx",
    "name": "主角",
    "region": { "left": 120, "top": 80, "right": 184, "bottom": 144 },
    "coordinateMode": "scale",
    "anchorX": "start",
    "anchorY": "start",
    "searchRadius": 3,
    "scaleSearchPercent": 2,
    "groups": [{
      "id": "grp_xxx",
      "name": "站立",
      "enabled": true,
      "minMatch": -1,
      "points": [{
        "dx": 12,
        "dy": 8,
        "reference": [255, 128, 0, 255],
        "tolerance": [24, 24, 24],
        "alphaMode": "ignore",
        "alphaTolerance": 40,
        "mustNot": false
      }]
    }]
  }],
  "replayCases": [{
    "id": "case_xxx",
    "name": "联合场景-1",
    "capturedAt": 1786531200000,
    "width": 1920,
    "height": 1080,
    "storage": "external",
    "pngDataUrl": "",
    "relativePath": "project.samples/abc-capture.png",
    "sha256": "...",
    "expectations": [
      { "objectId": "obj_xxx", "expectedGroupId": "grp_xxx" },
      { "objectId": "obj_other", "expectedGroupId": null }
    ],
    "tags": ["night", "boss"]
  }]
}
```

## 匹配与适配语义

1. `fixed` 要求回放/实时帧与基准尺寸一致。
2. `scale` 分别按宽高比例缩放区域和点坐标。
3. `anchor` 保持区域尺寸，按 start/center/end 吸附到新客户区。
4. 适配后可在 `searchRadius` 邻域和 `scaleSearchPercent` 百分比范围搜索；返回最先命中的最近候选，未命中则返回诊断得分最高的候选。
5. 常规点的 RGB 各通道差值不得超过容差；`alphaMode=match` 时 alpha 也必须通过。
6. 排除点必须不匹配参考色。组命中要求常规点通过数达到 `minMatch`（-1 表示全部），且所有排除点通过。
7. 一个对象的任意启用组命中即表示对象存在。回放指定某个状态时，仅该状态单独命中才通过；同时命中多个状态会报告状态歧义。

## 样本存储

- `embedded`：PNG Data URL 位于 `pngDataUrl`。单张编码不超过 24 MiB，总编码不超过 64 MiB。
- `external`：PNG 位于项目文件旁的受管 `<项目名>.samples` 目录，`relativePath` 不得为绝对路径或包含父目录跳转。
- 外置文件加载时校验 `sha256`。便携导出会把外置样本嵌入一个 JSON；超过内嵌上限时应连同 `.samples` 目录一起交付。

## 版本兼容

- v4：坐标适配、位置/缩放搜索、多对象回放期望、标签和外置样本。
- v3：可启停视觉状态和单对象内嵌回放。
- v2：LTRB 区域与客户区基准尺寸。
- v1：历史 `x/y/w/h` 区域。

应用可读取 v1～v4，保存时迁移到 v4。消费端必须拒绝高于自身支持范围的版本。完整约束见 [project-v4.schema.json](project-v4.schema.json)。
