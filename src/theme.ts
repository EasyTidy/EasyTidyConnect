import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * 界面风格：
 * - fluent：WinUI 3（实色 Mica 基底）
 * - mica：WinUI 3 + 真实的 Windows 11 云母材质（窗口透明，系统绘制背景）
 * - glass：iOS 26 风格的 Liquid Glass
 */
export type ThemeStyle = "fluent" | "mica" | "glass";
/** 明暗：跟随系统 / 浅色 / 深色。 */
export type ThemeMode = "system" | "light" | "dark";
export type Theme = { style: ThemeStyle; mode: ThemeMode };

const STORAGE_KEY = "connect.theme";
const DEFAULT_THEME: Theme = { style: "fluent", mode: "system" };
const STYLES: ThemeStyle[] = ["fluent", "mica", "glass"];
const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");
const inTauri = () => "__TAURI_INTERNALS__" in window;

export function loadTheme(): Theme {
  try {
    const saved = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "null");
    return {
      style: STYLES.includes(saved?.style) ? saved.style : DEFAULT_THEME.style,
      mode: saved?.mode === "light" || saved?.mode === "dark" ? saved.mode : "system",
    };
  } catch {
    return DEFAULT_THEME;
  }
}

const isDark = (theme: Theme) => theme.mode === "dark" || (theme.mode === "system" && darkQuery().matches);

/** 把主题写到 <html data-style data-mode data-mica>，CSS 据此切换整套变量。 */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.dataset.style = theme.style === "glass" ? "glass" : "fluent";
  root.dataset.mode = isDark(theme) ? "dark" : "light";
  if (theme.style !== "mica") delete root.dataset.mica;
}

/**
 * 同步原生窗口：标题栏明暗、云母材质。云母只有在系统确认生效后才把页面背景设为透明，
 * Windows 10 等不支持的系统会保持实色背景。
 */
async function syncWindow(theme: Theme): Promise<boolean> {
  if (!inTauri()) return false;
  try {
    return await invoke<boolean>("set_window_appearance", {
      mica: theme.style === "mica",
      dark: isDark(theme),
    });
  } catch {
    return false;
  }
}

/** 主题状态 + 持久化；跟随系统时监听系统明暗变化。返回值第三项表示云母是否可用/生效。 */
export function useTheme(): [Theme, (theme: Theme) => void, boolean | null] {
  const [theme, setTheme] = useState<Theme>(loadTheme);
  const [micaActive, setMicaActive] = useState<boolean | null>(null);

  useEffect(() => {
    let cancelled = false;
    const apply = () => {
      applyTheme(theme);
      void syncWindow(theme).then((active) => {
        if (cancelled) return;
        if (theme.style === "mica") {
          setMicaActive(active);
          if (active) document.documentElement.dataset.mica = "on";
          else delete document.documentElement.dataset.mica;
        }
      });
    };
    apply();
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(theme));
    if (theme.mode !== "system") {
      return () => {
        cancelled = true;
      };
    }
    const query = darkQuery();
    query.addEventListener("change", apply);
    return () => {
      cancelled = true;
      query.removeEventListener("change", apply);
    };
  }, [theme.style, theme.mode]);

  return [theme, setTheme, micaActive];
}
