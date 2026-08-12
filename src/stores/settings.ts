/**
 * 设置 store：主题（Light / Dark / Follow system）。
 * 仅保存跨页面全局偏好，故使用 Pinia。
 */
import { defineStore } from "pinia";
import { ref, watch } from "vue";

export type ThemeMode = "light" | "dark" | "system";

const STORAGE_KEY = "imagesitter.theme";

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export const useSettingsStore = defineStore("settings", () => {
  const theme = ref<ThemeMode>(
    (localStorage.getItem(STORAGE_KEY) as ThemeMode) || "system",
  );

  function apply() {
    const dark = theme.value === "dark" || (theme.value === "system" && systemPrefersDark());
    document.documentElement.classList.toggle("dark", dark);
  }

  function setTheme(mode: ThemeMode) {
    theme.value = mode;
    localStorage.setItem(STORAGE_KEY, mode);
    apply();
  }

  watch(theme, apply, { immediate: true });

  // 跟随系统时监听系统主题变化
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => {
      if (theme.value === "system") apply();
    });

  return { theme, setTheme };
});
