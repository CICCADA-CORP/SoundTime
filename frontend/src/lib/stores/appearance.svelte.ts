export type AppearanceMode = "light" | "dark" | "system";

// Keep existing preferences set on the account screen before appearance became app-wide.
const storageKey = "soundtime_auth_appearance";
let mode = $state<AppearanceMode>("system");

function parse(value: string | null): AppearanceMode {
  return value === "light" || value === "dark" ? value : "system";
}

function apply(value: AppearanceMode) {
  mode = value;
  const dark = value === "dark" || (value === "system" && window.matchMedia?.("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.appearance = dark ? "dark" : "light";
}

export const appearance = {
  get mode() { return mode; },
  choose(value: AppearanceMode) {
    apply(value);
    try {
      localStorage.setItem(storageKey, value);
    } catch {
      // The preference still applies to the current session.
    }
  },
  listen() {
    try {
      apply(parse(localStorage.getItem(storageKey)));
    } catch {
      apply("system");
    }
    const system = window.matchMedia?.("(prefers-color-scheme: dark)");
    const syncSystem = () => { if (mode === "system") apply(mode); };
    const syncStorage = (event: StorageEvent) => {
      if (event.key === storageKey || event.key === null) apply(parse(event.newValue));
    };
    system?.addEventListener("change", syncSystem);
    window.addEventListener("storage", syncStorage);
    return () => {
      system?.removeEventListener("change", syncSystem);
      window.removeEventListener("storage", syncStorage);
    };
  },
};
