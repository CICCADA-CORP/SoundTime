import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, screen } from "@testing-library/svelte";
import { flushSync, mount, unmount } from "svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import LoginAppearance from "./LoginAppearance.svelte";

afterEach(() => {
  localStorage.removeItem("soundtime_auth_appearance");
  vi.unstubAllGlobals();
});

it("resolves the system preference, preserves an explicit choice and restores it on the next visit", async () => {
  setLocale("en");
  let systemDark = true;
  const system = new EventTarget();
  Object.defineProperty(system, "matches", { get: () => systemDark });
  vi.stubGlobal("matchMedia", vi.fn(() => system));
  let dark = false;
  const props = { get dark() { return dark; }, set dark(value: boolean) { dark = value; } };
  const component = mount(LoginAppearance, { target: document.body, props });
  flushSync();
  expect(dark).toBe(true);
  expect(screen.getByRole("radio", { name: "Use system appearance" })).toBeChecked();
  systemDark = false;
  system.dispatchEvent(new Event("change"));
  expect(dark).toBe(false);
  await fireEvent.click(screen.getByRole("radio", { name: "Dark mode" }));
  expect(dark).toBe(true);
  expect(localStorage.getItem("soundtime_auth_appearance")).toBe("dark");
  system.dispatchEvent(new Event("change"));
  expect(dark).toBe(true);
  await unmount(component);

  dark = false;
  const restored = mount(LoginAppearance, { target: document.body, props });
  flushSync();
  expect(dark).toBe(true);
  expect(screen.getByRole("radio", { name: "Dark mode" })).toBeChecked();
  await fireEvent.click(screen.getByRole("radio", { name: "Light mode" }));
  expect(dark).toBe(false);
  await unmount(restored);
});
