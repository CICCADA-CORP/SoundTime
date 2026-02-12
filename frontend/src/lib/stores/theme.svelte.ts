import { api } from "$lib/api";
import type { Theme } from "$lib/types";

let activeTheme = $state<Theme | null>(null);
let loading = $state(true);
let safeMode = $state(false);

/** CSS link element managed by this store. */
let linkEl: HTMLLinkElement | null = null;

/** Check if safe mode is requested via URL param or localStorage. */
function checkSafeMode(): boolean {
  if (typeof window === "undefined") return false;
  const params = new URLSearchParams(window.location.search);
  if (params.get("theme") === "default") return true;
  if (localStorage.getItem("soundtime_theme_safe") === "1") return true;
  return false;
}

/** Inject a <link> stylesheet for the active theme. */
function injectTheme(): void {
  if (typeof document === "undefined") return;
  if (linkEl) return; // already injected
  linkEl = document.createElement("link");
  linkEl.rel = "stylesheet";
  linkEl.href = `/api/themes/active.css?v=${Date.now()}`;
  linkEl.id = "soundtime-theme";
  document.head.appendChild(linkEl);
}

/** Remove the injected theme stylesheet. */
function removeTheme(): void {
  if (linkEl) {
    linkEl.remove();
    linkEl = null;
  }
}

/** Initialize the theme store: check safe mode, fetch active theme, inject CSS. */
async function init(): Promise<void> {
  loading = true;
  safeMode = checkSafeMode();

  if (safeMode) {
    removeTheme();
    activeTheme = null;
    loading = false;
    return;
  }

  try {
    const resp = await fetch("/api/themes/active");
    if (resp.ok) {
      activeTheme = await resp.json();
    } else {
      activeTheme = null;
    }
  } catch {
    activeTheme = null;
  }

  if (activeTheme) {
    injectTheme();
  } else {
    removeTheme();
  }

  loading = false;
}

/** Re-fetch and re-inject the active theme (e.g., after admin changes). */
async function refresh(): Promise<void> {
  removeTheme();
  await init();
}

export function getThemeStore() {
  return {
    get activeTheme() { return activeTheme; },
    get loading() { return loading; },
    get safeMode() { return safeMode; },
    init,
    refresh,
    injectTheme,
    removeTheme,
  };
}
