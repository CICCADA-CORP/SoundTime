import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within, waitFor } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import AdminPage from "./+page.svelte";
import { getTaskStore } from "$lib/stores/tasks.svelte";
import type { StorageTaskStatus } from "$lib/types";

vi.mock("$lib/stores/auth.svelte", () => ({
  getAuthStore: () => ({ isAdmin: true, loading: false })
}));

afterEach(() => {
  getTaskStore().dismiss();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("confirms only the named empty volume and displays an API failure", async () => {
  setLocale("fr");
  const requests = vi.fn(async (url: string, options: RequestInit) => {
    if (options.method === "POST") {
      return Response.json({ error: "Volume indisponible" }, { status: 503 });
    }
    if (url === "/api/admin/storage/task-status") {
      return Response.json({
        status: "completed",
        result: { kind: "sync", scanned: 12, imported: 0, moved: 0, removed: 0, skipped: 12, errors: [], empty_sources: ["network-nfs"] }
      });
    }
    return Response.json(null);
  });
  vi.stubGlobal("fetch", requests);
  render(AdminPage);
  await fireEvent.click(screen.getByText("Synchronisation", { selector: "button" }));
  await screen.findByText(/Aucun fichier audio trouvé dans network-nfs/);
  const cleanup = screen.getByText("Retirer les pistes manquantes", { selector: "button" });
  await fireEvent.click(cleanup);
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain("network-nfs");
  await fireEvent.click(within(dialog).getByRole("button", { name: "Annuler" }));
  await waitFor(() => expect(document.activeElement).toBe(cleanup));
  expect(requests.mock.calls.filter(([, options]) => options.method === "POST")).toHaveLength(0);
  await fireEvent.click(cleanup);
  await fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Retirer les pistes manquantes" }));
  await screen.findByText("Volume indisponible");
  expect(requests.mock.calls.filter(([, options]) => options.method === "POST")).toEqual([
    ["/api/admin/storage/sync?prune_empty=network-nfs", expect.objectContaining({ method: "POST" })]
  ]);
});


function openStorageTab(tab: string) {
  setLocale("en");
  render(AdminPage);
  return fireEvent.click(screen.getByRole("button", { name: tab }));
}

it.each([
  ["local", 0, "0 B", "/catalog/audio"],
  ["local", 2048, "2.0 KB", "/catalog/audio"],
  ["s3", 3 * 1024 ** 2, "3.0 MB", "music-bucket"],
  ["s3", 2 * 1024 ** 3, "2.00 GB", "music-bucket"],
])("shows the configured %s storage and formats %s bytes", async (backend, totalBytes, expectedSize, location) => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(
    url === "/api/admin/storage/status" ? {
      backend, total_tracks: 27, total_size_bytes: totalBytes,
      storage_path_or_bucket: location, remote_track_count: 8, remote_available_count: 7,
    } : null
  )));
  await openStorageTab("Storage");
  expect(await screen.findByText(expectedSize)).toBeTruthy();
  expect(screen.getByText(location)).toBeTruthy();
  expect(screen.getByText(backend === "s3" ? "Bucket" : "Path")).toBeTruthy();
  expect(screen.getByText(backend === "s3" ? /S3 storage configured/ : /Local storage\./)).toBeTruthy();
});

it("lists missing tracks and scanner errors, then opens reconciliation", async () => {
  const statuses: StorageTaskStatus[] = [
    { status: "completed", result: {
      kind: "integrity", total_checked: 27, healthy: 26,
      missing: [{ track_id: "a", title: "Missing recording", file_path: "a.flac" }], errors: ["Unreadable volume"],
    } },
    { status: "completed", result: {
      kind: "sync", scanned: 26, imported: 1, moved: 2, removed: 0,
      skipped: 23, errors: ["Could not import b.flac"], empty_sources: [],
    } },
  ];
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(
    url.endsWith("/task-status") ? statuses.shift() : null
  )));
  await openStorageTab("Integrity");
  expect(await screen.findByText("Missing recording")).toBeTruthy();
  expect(screen.getByText("Unreadable volume")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: /Reconcile missing files/ }));
  expect(await screen.findByText("Could not import b.flac")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Remove missing tracks" })).toBeNull();
});

it("recovers a scan after the start request times out and displays its final error", async () => {
  const statuses: StorageTaskStatus[] = [
    { status: "idle" },
    { status: "running", progress: { processed: 2, total: null } },
    { status: "running", progress: { processed: 5, total: 20 } },
    { status: "error", message: "Disk disconnected while scanning" },
  ];
  const fetchMock = vi.fn(async (url: string, options: RequestInit) => {
    if (options.method === "POST") return Response.json({ error: "Gateway timeout" }, { status: 504 });
    return Response.json(url.endsWith("/task-status") ? statuses.shift() : null);
  });
  vi.stubGlobal("fetch", fetchMock);
  await openStorageTab("Integrity");
  await fireEvent.click(await screen.findByRole("button", { name: "Run check" }));
  expect(await screen.findByText("5 / 20")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Checking…" }).hasAttribute("disabled")).toBe(true);
  await screen.findByText("Disk disconnected while scanning", {}, { timeout: 6000 });
  expect(fetchMock.mock.calls.filter(([, options]) => options.method === "POST")).toHaveLength(1);
});

it("reattaches to an existing sync without starting another task, then allows a new sync", async () => {
  const statuses: StorageTaskStatus[] = [
    { status: "running", progress: { processed: 8, total: null } },
    { status: "running", progress: { processed: 9, total: null } },
    { status: "idle" },
    { status: "completed", result: {
      kind: "sync", imported: 0, moved: 0, removed: 0, scanned: 12, skipped: 12, empty_sources: [], errors: [],
    } },
  ];
  const fetchMock = vi.fn(async (url: string, options: RequestInit) => {
    if (options.method === "POST") return new Response(null, { status: 202 });
    return Response.json(url.endsWith("/task-status") ? statuses.shift() : null);
  });
  vi.stubGlobal("fetch", fetchMock);
  await openStorageTab("Synchronization");
  expect(await screen.findByText("9")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Synchronizing…" }).hasAttribute("disabled")).toBe(true);
  expect(fetchMock.mock.calls.filter(([, options]) => options.method === "POST")).toHaveLength(0);
  await fireEvent.click(await screen.findByRole("button", { name: "Run synchronization" }, { timeout: 6000 }));
  expect(await screen.findByText("Files scanned")).toBeTruthy();
  expect(fetchMock.mock.calls.filter(([, options]) => options.method === "POST")).toEqual([
    ["/api/admin/storage/sync", expect.objectContaining({ method: "POST" })],
  ]);
});

it("opens the storage tools through the compact section selector", async () => {
  setLocale("en");
  vi.stubGlobal("fetch", vi.fn(async (url: string) =>
    Response.json(url.endsWith("/task-status") ? { status: "idle" } : null)
  ));
  render(AdminPage);
  await fireEvent.change(screen.getByRole("combobox"), { target: { value: "sync" } });
  expect(await screen.findByRole("heading", { name: "Storage synchronization" })).toBeTruthy();
  await fireEvent.change(screen.getByRole("combobox"), { target: { value: "integrity" } });
  expect(await screen.findByRole("heading", { name: "Integrity check" })).toBeTruthy();
});
