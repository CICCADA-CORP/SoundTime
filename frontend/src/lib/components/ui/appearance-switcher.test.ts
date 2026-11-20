import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import { appearance } from "$lib/stores/appearance.svelte";
import AppearanceSwitcher from "./appearance-switcher.svelte";

afterEach(() => {
  localStorage.removeItem("soundtime_auth_appearance");
  delete document.documentElement.dataset.appearance;
  vi.unstubAllGlobals();
});

it("carries the account preference across the app and follows system and tab changes", async () => {
  setLocale("en");
  const system = new EventTarget();
  let dark = false;
  Object.defineProperty(system, "matches", { get: () => dark });
  vi.stubGlobal("matchMedia", () => system);
  localStorage.setItem("soundtime_auth_appearance", "dark");
  const stop = appearance.listen();
  try {
    render(AppearanceSwitcher);
    expect(document.documentElement.dataset.appearance).toBe("dark");
    expect(screen.getByRole("radio", { name: "Dark mode" })).toBeChecked();
    await fireEvent.click(screen.getByRole("radio", { name: "Use system appearance" }));
    expect(localStorage.getItem("soundtime_auth_appearance")).toBe("system");
    expect(document.documentElement.dataset.appearance).toBe("light");
    dark = true;
    system.dispatchEvent(new Event("change"));
    expect(document.documentElement.dataset.appearance).toBe("dark");
    await fireEvent(window, new StorageEvent("storage", { key: "soundtime_auth_appearance", newValue: "light" }));
    expect(screen.getByRole("radio", { name: "Light mode" })).toBeChecked();
    system.dispatchEvent(new Event("change"));
    expect(document.documentElement.dataset.appearance).toBe("light");
  } finally {
    stop();
  }
});
