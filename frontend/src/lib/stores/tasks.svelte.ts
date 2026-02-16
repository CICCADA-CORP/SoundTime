import { api } from "$lib/api";
import type { StorageTaskStatus, TaskProgress } from "$lib/types";

// ─── Module-level state ────────────────────────────────────────────

let taskType = $state<"sync" | "integrity" | null>(null);
let status = $state<StorageTaskStatus | null>(null);
let polling = $state(false);
let pollTimer: ReturnType<typeof setTimeout> | null = null;
let retryCount = $state(0);

const POLL_INTERVAL = 1500;
const RETRY_INTERVAL = 5000;
const MAX_RETRIES = 3;

// ─── Private functions ─────────────────────────────────────────────

async function poll() {
  if (!polling) return;

  try {
    const result = await api.get<StorageTaskStatus>("/admin/storage/task-status");
    status = result;
    retryCount = 0;

    if (result.status === "running") {
      pollTimer = setTimeout(poll, POLL_INTERVAL);
    } else {
      // completed, error, or idle — stop polling but keep status visible
      polling = false;
    }
  } catch {
    retryCount++;
    if (retryCount < MAX_RETRIES) {
      pollTimer = setTimeout(poll, RETRY_INTERVAL);
    } else {
      polling = false;
      status = { status: "error", message: "Lost connection to server. The task may still be running." };
    }
  }
}

function startPolling(type: "sync" | "integrity") {
  stopPolling();
  taskType = type;
  polling = true;
  retryCount = 0;
  status = { status: "running", progress: { processed: 0, total: null } };
  poll();
}

function stopPolling() {
  if (pollTimer) {
    clearTimeout(pollTimer);
    pollTimer = null;
  }
  polling = false;
}

function dismiss() {
  stopPolling();
  taskType = null;
  status = null;
}

async function checkForRunningTask() {
  try {
    const result = await api.get<StorageTaskStatus>("/admin/storage/task-status");
    if (result.status === "running") {
      status = result;
      taskType = taskType ?? "sync";
      polling = true;
      retryCount = 0;
      poll();
    }
  } catch {
    // silently ignore — we're just checking
  }
}

// ─── Public getter factory ─────────────────────────────────────────

export function getTaskStore() {
  return {
    /** Whether a task is currently being tracked (running or just completed) */
    get isActive(): boolean {
      return status !== null && status.status !== "idle";
    },
    /** Whether the task is currently running */
    get isRunning(): boolean {
      return status?.status === "running";
    },
    /** The task type: 'sync' | 'integrity' | null */
    get taskType(): "sync" | "integrity" | null {
      return taskType;
    },
    /** Current progress (processed/total) */
    get progress(): TaskProgress | null {
      if (status?.status === "running") return status.progress;
      return null;
    },
    /** Last completed/error status */
    get lastStatus(): StorageTaskStatus | null {
      return status;
    },
    /** Start polling for a specific task type */
    startPolling,
    /** Stop polling and clear state */
    dismiss,
    /** Check if a task is already running (e.g., on page load) */
    checkForRunningTask,
  };
}
