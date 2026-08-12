import type { FeatureGroup, ObjectSpec, Region } from "./types";

export interface ResolvedObject {
  region: Region;
  groups: FeatureGroup[];
}

const round = (value: number) => Math.max(0, Math.round(value));

/** 把基准客户区中的对象规则解析到当前帧坐标系。 */
export function resolveObjectForFrame(
  object: ObjectSpec,
  baseWidth: number,
  baseHeight: number,
  frameWidth: number,
  frameHeight: number,
): ResolvedObject {
  if (
    object.coordinateMode === "fixed" ||
    baseWidth <= 0 ||
    baseHeight <= 0 ||
    frameWidth <= 0 ||
    frameHeight <= 0 ||
    (baseWidth === frameWidth && baseHeight === frameHeight)
  ) {
    return { region: structuredClone(object.region), groups: structuredClone(object.groups) };
  }

  if (object.coordinateMode === "anchor") {
    const shift = (delta: number, anchor: "start" | "center" | "end") =>
      anchor === "end" ? delta : anchor === "center" ? Math.round(delta / 2) : 0;
    return {
      region: {
        ...object.region,
        x: Math.max(0, object.region.x + shift(frameWidth - baseWidth, object.anchorX)),
        y: Math.max(0, object.region.y + shift(frameHeight - baseHeight, object.anchorY)),
      },
      groups: structuredClone(object.groups),
    };
  }

  const sx = frameWidth / baseWidth;
  const sy = frameHeight / baseHeight;
  const region: Region = {
    x: round(object.region.x * sx),
    y: round(object.region.y * sy),
    w: Math.max(1, round(object.region.w * sx)),
    h: Math.max(1, round(object.region.h * sy)),
  };
  return {
    region,
    groups: object.groups.map((group) => ({
      ...structuredClone(group),
      points: group.points.map((point) => ({
        ...structuredClone(point),
        dx: Math.min(region.w - 1, round(point.dx * sx)),
        dy: Math.min(region.h - 1, round(point.dy * sy)),
      })),
    })),
  };
}

export function frameSizeCompatible(
  object: ObjectSpec | null,
  baseWidth: number,
  baseHeight: number,
  frameWidth: number,
  frameHeight: number,
): boolean {
  if (!object || baseWidth <= 0 || baseHeight <= 0 || frameWidth <= 0 || frameHeight <= 0) return true;
  return object.coordinateMode !== "fixed" || (baseWidth === frameWidth && baseHeight === frameHeight);
}
