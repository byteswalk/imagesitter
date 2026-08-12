import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/** 合并 Tailwind 类名，shadcn-vue 标准工具函数。 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** 生成短随机 ID，用于对象与特征组。 */
export function shortId(prefix: string): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 8)}${Date.now()
    .toString(36)
    .slice(-4)}`;
}

/** 把 [r,g,b,a] 转为十六进制颜色字符串。 */
export function rgbaToHex(rgba: [number, number, number, number]): string {
  const [r, g, b] = rgba;
  return `#${[r, g, b]
    .map((value) => value.toString(16).padStart(2, "0"))
    .join("")}`;
}
