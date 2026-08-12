/** 只保留 Windows NSIS 构建需要的图标，避免 `tauri icon` 生成移动端和商店冗余资产。 */
import { readdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const icons = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
const keep = new Set(["32x32.png", "128x128.png", "128x128@2x.png", "icon.ico"]);

for (const entry of readdirSync(icons, { withFileTypes: true })) {
  if (!keep.has(entry.name)) {
    rmSync(join(icons, entry.name), { recursive: true, force: true });
  }
}

console.log(`Windows icons retained: ${[...keep].join(", ")}`);
