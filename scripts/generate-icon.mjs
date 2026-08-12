/**
 * 生成 ImageSitter 应用图标源图（1024x1024 PNG），
 * 随后运行 `pnpm tauri icon scripts/icon-source.png` 生成全尺寸图标。
 * 纯 Node 实现：zlib 压缩 + 手工 PNG 分块，无第三方依赖。
 */
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SIZE = 1024;
const RADIUS = 224;

// 对角渐变端点（靛蓝 -> 紫）
const GRAD_FROM = [79, 70, 229];
const GRAD_TO = [147, 51, 234];
const CYAN = [34, 211, 238];
const WHITE = [255, 255, 255];

/** 圆角矩形 SDF：返回像素是否在形状内 */
function inRoundedRect(x, y, left, top, right, bottom, radius) {
  if (x < left || y < top || x > right || y > bottom) return false;
  const cx = Math.max(left + radius, Math.min(x, right - radius));
  const cy = Math.max(top + radius, Math.min(y, bottom - radius));
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= radius * radius;
}

function inSquare(x, y, cx, cy, half) {
  return Math.abs(x - cx) <= half && Math.abs(y - cy) <= half;
}

const buffer = Buffer.alloc(SIZE * SIZE * 4);

const center = SIZE / 2;
const lineHalf = 7; // 十字线半宽
const gapHalf = 96; // 中心留空半区
const pixelHalf = 56; // 像素方块半边长
const orbit = 240; // 四个像素块中心到画面中心距离

for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const offset = (y * SIZE + x) * 4;
    if (!inRoundedRect(x, y, 8, 8, SIZE - 9, SIZE - 9, RADIUS)) {
      buffer[offset + 3] = 0;
      continue;
    }
    // 对角渐变
    const t = (x + y) / (2 * SIZE);
    let r = GRAD_FROM[0] + (GRAD_TO[0] - GRAD_FROM[0]) * t;
    let g = GRAD_FROM[1] + (GRAD_TO[1] - GRAD_FROM[1]) * t;
    let b = GRAD_FROM[2] + (GRAD_TO[2] - GRAD_FROM[2]) * t;

    // 十字准星线（中心留空）
    const onHLine =
      Math.abs(y - center) <= lineHalf && Math.abs(x - center) > gapHalf;
    const onVLine =
      Math.abs(x - center) <= lineHalf && Math.abs(y - center) > gapHalf;
    if (onHLine || onVLine) {
      r = r * 0.25 + WHITE[0] * 0.75;
      g = g * 0.25 + WHITE[1] * 0.75;
      b = b * 0.25 + WHITE[2] * 0.75;
    }

    // 四个方位的白色像素块
    const satellites = [
      [center, center - orbit],
      [center, center + orbit],
      [center - orbit, center],
      [center + orbit, center],
    ];
    for (const [sx, sy] of satellites) {
      if (inSquare(x, y, sx, sy, pixelHalf)) {
        r = WHITE[0];
        g = WHITE[1];
        b = WHITE[2];
      }
    }

    // 中心焦点像素（青色）
    if (inSquare(x, y, center, center, pixelHalf + 8)) {
      r = CYAN[0];
      g = CYAN[1];
      b = CYAN[2];
    }

    buffer[offset] = Math.round(r);
    buffer[offset + 1] = Math.round(g);
    buffer[offset + 2] = Math.round(b);
    buffer[offset + 3] = 255;
  }
}

// ---- PNG 编码 ----
const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(chunk) {
  let crc = 0xffffffff;
  for (const byte of chunk) {
    crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function makeChunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

// 每行前置 filter 字节 0
const scanlines = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  const rowStart = y * (SIZE * 4 + 1);
  scanlines[rowStart] = 0;
  buffer.copy(scanlines, rowStart + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // 位深
ihdr[9] = 6; // RGBA
ihdr[10] = 0;
ihdr[11] = 0;
ihdr[12] = 0;

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  makeChunk("IHDR", ihdr),
  makeChunk("IDAT", deflateSync(scanlines, { level: 9 })),
  makeChunk("IEND", Buffer.alloc(0)),
]);

const outputPath = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "scripts",
  "icon-source.png",
);
writeFileSync(outputPath, png);
console.log(`icon written: ${outputPath}`);
