import { createServer } from "vite";

const server = await createServer({
  appType: "custom",
  logLevel: "silent",
  server: { middlewareMode: true },
});

try {
  const { parseProject, serializeProject } = await server.ssrLoadModule(
    "/src/stores/project.ts",
  );
  const { resolveObjectForFrame, frameSizeCompatible } = await server.ssrLoadModule(
    "/src/lib/matching.ts",
  );
  const legacy = {
    version: 1,
    target: { windowTitle: "demo", className: "Demo", processId: 7 },
    objects: [
      {
        id: "obj_1",
        name: "对象",
        region: { x: 2, y: 3, w: 4, h: 5 },
        groups: [
          {
            id: "grp_1",
            name: "形态",
            minMatch: -1,
            points: [
              {
                dx: 1,
                dy: 1,
                reference: [1, 2, 3, 255],
                tolerance: [4, 5, 6],
                alphaMode: "ignore",
                alphaTolerance: 40,
                mustNot: false,
              },
            ],
          },
        ],
      },
    ],
  };

  const parsed = parseProject(legacy);
  if (
    parsed.version !== 4 ||
    parsed.objects[0].region.w !== 4 ||
    parsed.target.frameWidth !== 0 ||
    parsed.objects[0].groups[0].enabled !== true ||
    parsed.replayCases.length !== 0 ||
    parsed.objects[0].coordinateMode !== "fixed"
  ) {
    throw new Error("v1 migration failed");
  }

  const saved = JSON.parse(serializeProject(parsed));
  if (
    saved.version !== 4 ||
    saved.objects[0].region.left !== 2 ||
    saved.objects[0].region.right !== 6 ||
    "x" in saved.objects[0].region
  ) {
    throw new Error("v2 serialization failed");
  }

  const earlyLtrb = structuredClone(legacy);
  earlyLtrb.objects[0].region = { left: 2, top: 3, right: 6, bottom: 8 };
  if (parseProject(earlyLtrb).objects[0].region.h !== 5) {
    throw new Error("legacy LTRB compatibility failed");
  }

  const invalidProjects = [
    { ...legacy, version: 5 },
    {
      ...legacy,
      objects: [
        {
          ...legacy.objects[0],
          groups: [
            {
              ...legacy.objects[0].groups[0],
              points: [
                { ...legacy.objects[0].groups[0].points[0], dx: 99 },
              ],
            },
          ],
        },
      ],
    },
  ];
  let rejected = 0;
  for (const invalid of invalidProjects) {
    try {
      parseProject(invalid);
    } catch {
      rejected += 1;
    }
  }
  if (rejected !== invalidProjects.length) {
    throw new Error("invalid project was accepted");
  }

  const withReplay = {
    ...saved,
    replayCases: [
      {
        id: "case_1",
        name: "存在样本",
        capturedAt: 1,
        width: 10,
        height: 10,
        pngDataUrl: "data:image/png;base64,AAAA",
        objectId: "obj_1",
        expectedGroupId: "grp_1",
      },
    ],
  };
  const replayParsed = parseProject(withReplay);
  if (
    replayParsed.replayCases[0].expectations[0].expectedGroupId !== "grp_1" ||
    replayParsed.replayCases[0].storage !== "embedded" ||
    replayParsed.objects[0].groups[0].enabled !== true
  ) {
    throw new Error("v3 replay/state parsing failed");
  }

  const brokenReplay = structuredClone(withReplay);
  brokenReplay.replayCases[0].expectedGroupId = "missing";
  try {
    parseProject(brokenReplay);
    throw new Error("invalid replay reference was accepted");
  } catch (error) {
    if (String(error).includes("was accepted")) throw error;
  }

  const external = structuredClone(saved);
  external.replayCases = [{
    id: "case_external",
    name: "外部样本",
    capturedAt: 2,
    width: 10,
    height: 10,
    storage: "external",
    pngDataUrl: "",
    relativePath: "project.samples/a.png",
    sha256: "abc",
    expectations: [{ objectId: "obj_1", expectedGroupId: null }],
    tags: ["night"],
  }];
  if (parseProject(external).replayCases[0].tags[0] !== "night") {
    throw new Error("v4 external sample parsing failed");
  }
  external.replayCases[0].relativePath = "../escape.png";
  try {
    parseProject(external);
    throw new Error("unsafe external path was accepted");
  } catch (error) {
    if (String(error).includes("was accepted")) throw error;
  }

  const scalable = structuredClone(parsed.objects[0]);
  scalable.coordinateMode = "scale";
  const scaled = resolveObjectForFrame(scalable, 10, 10, 20, 30);
  if (scaled.region.x !== 4 || scaled.region.y !== 9 || scaled.region.w !== 8 || scaled.region.h !== 15) {
    throw new Error("scale coordinate adaptation failed");
  }
  const anchored = structuredClone(parsed.objects[0]);
  anchored.coordinateMode = "anchor";
  anchored.anchorX = "end";
  anchored.anchorY = "center";
  const moved = resolveObjectForFrame(anchored, 10, 10, 20, 30);
  if (moved.region.x !== 12 || moved.region.y !== 13) {
    throw new Error("anchor coordinate adaptation failed");
  }
  if (frameSizeCompatible(parsed.objects[0], 10, 10, 20, 20)) {
    throw new Error("fixed coordinate mismatch was accepted");
  }

  console.log("project-format checks: 14 passed");
} finally {
  await server.close();
}
