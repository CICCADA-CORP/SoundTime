import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import AdminPage from "./+page.svelte";

vi.mock("$lib/stores/auth.svelte", () => ({
  getAuthStore: () => ({ isAdmin: true, loading: false })
}));

afterEach(() => {
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
  const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
  await fireEvent.click(cleanup);
  expect(confirm).toHaveBeenCalledWith(expect.stringContaining("network-nfs"));
  expect(requests.mock.calls.filter(([, options]) => options.method === "POST")).toHaveLength(0);
  await fireEvent.click(cleanup);
  await screen.findByText("Volume indisponible");
  expect(requests.mock.calls.filter(([, options]) => options.method === "POST")).toEqual([
    ["/api/admin/storage/sync?prune_empty=network-nfs", expect.objectContaining({ method: "POST" })]
  ]);
});
