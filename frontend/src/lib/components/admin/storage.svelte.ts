import { api } from "$lib/api";
import { t } from "$lib/i18n/index.svelte";
import { getTaskStore } from "$lib/stores/tasks.svelte";
import type { IntegrityReport, StorageStatus, StorageTaskStatus, SyncReport } from "$lib/types";

export type StorageTab = "storage" | "sync" | "integrity";
type TaskKind = Exclude<StorageTab, "storage">;

export function createStorageSection() {
  const tasks = getTaskStore();
  let overview = $state.raw<StorageStatus | null>(null);
  let syncReport = $state.raw<SyncReport | null>(null);
  let integrityReport = $state.raw<IntegrityReport | null>(null);
  let pending = $state<TaskKind | null>(null);
  let error = $state("");
  let loadRequest = 0;

  function readResult(status: StorageTaskStatus | null) {
    if (status?.status === "completed") {
      if (status.result.kind === "sync") syncReport = status.result;
      else integrityReport = status.result;
    }
    if (status?.status === "error") error = status.message;
  }

  $effect(() => { readResult(tasks.lastStatus); });

  async function load(tab: StorageTab) {
    const request = ++loadRequest;
    error = "";
    if (tab === "storage") {
      const status = await api.get<StorageStatus>("/admin/storage/status");
      if (request === loadRequest) overview = status;
      return;
    }
    if (tasks.isRunning) return;
    const status = await api.get<StorageTaskStatus>("/admin/storage/task-status");
    if (request !== loadRequest || tasks.isRunning) return;
    if (status.status === "running") tasks.startPolling(tab);
    else readResult(status);
  }

  async function run(kind: TaskKind, emptySources: string[] = []) {
    if (pending || tasks.isRunning) return;
    pending = kind;
    error = "";
    const query = emptySources.length ? "?prune_empty=" + encodeURIComponent(emptySources.join(",")) : "";
    const path = kind === "integrity" ? "/admin/storage/integrity-check" : "/admin/storage/sync" + query;
    try {
      await api.post(path);
      tasks.startPolling(kind);
    } catch (cause) {
      // A timed-out POST can still start the task server-side.
      const status = await api.get<StorageTaskStatus>("/admin/storage/task-status").catch(() => null);
      if (status?.status === "running") tasks.startPolling(kind);
      else error = cause instanceof Error ? cause.message : t("admin.unknownError");
    } finally {
      pending = null;
    }
  }

  return {
    get overview() { return overview; },
    get syncReport() { return syncReport; },
    get integrityReport() { return integrityReport; },
    get busy() { return pending !== null || tasks.isRunning; },
    get running() { return pending ?? (tasks.isRunning ? tasks.taskType : null); },
    get progress() { return tasks.progress; },
    get error() { return error; },
    load,
    run,
  };
}
