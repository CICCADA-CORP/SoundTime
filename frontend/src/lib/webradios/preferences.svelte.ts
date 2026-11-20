import { api } from "$lib/api";
import { live } from "./live.svelte";
import { podcast } from "./podcast.svelte";
import type { WebradioStatus } from "./types";

const guestKey = "soundtime_webradios_guest";
const changeKey = "soundtime_webradios_changed";
let member = $state<string | null>();
let status = $state.raw<WebradioStatus | null>(null);
let guestEnabled = $state(true);
let failed = $state(false);
let revision = 0;
let pending: Promise<void> | null = null;
let saving: number | undefined;

function stopBroadcasts() {
  live.stop();
  podcast.stop();
}

async function verifyPlayback() {
  const id = member;
  if (id === undefined) return false;
  await refresh(id);
  return member === id && allowed();
}

function readGuest() {
  try { return localStorage.getItem(guestKey) !== "false"; }
  catch { return guestEnabled; }
}
function allowed() { return status?.enabled === true && (member != null || guestEnabled); }

function refresh(id: string | null) {
  if (member !== id) {
    if (member !== undefined) stopBroadcasts();
    revision++;
    pending = null;
    status = null;
    member = id;
  }
  if (saving === revision) return Promise.resolve();
  if (pending) return pending;
  const request = ++revision;
  if (id === null) guestEnabled = readGuest();
  failed = false;
  pending = api.get<WebradioStatus>("/webradios/status", AbortSignal.timeout(10_000)).then(value => {
    if (revision !== request) return;
    if (!value || typeof value.enabled !== "boolean" || typeof value.instance_enabled !== "boolean" || typeof value.user_enabled !== "boolean") {
      failed = true;
      status = null;
    } else {
      status = value;
    }
    if (!allowed()) stopBroadcasts();
  }, () => {
    if (revision !== request) return;
    failed = true;
    // A direct external stream outlives its resolution response. Revalidate
    // permission on a bounded interval and fail closed if it cannot be checked.
    status = null;
    stopBroadcasts();
  }).finally(() => { if (revision === request) pending = null; });
  return pending;
}

function notify() {
  try { localStorage.setItem(changeKey, crypto.randomUUID?.() ?? String(Date.now())); } catch { /* private browsing */ }
}

async function setEnabled(enabled: boolean, id: string | null) {
  if (id !== member || pending || !status) await refresh(id);
  if (id !== member) return;
  const request = ++revision;
  pending = null;
  if (id === null) {
    guestEnabled = enabled;
    try { localStorage.setItem(guestKey, String(enabled)); } catch { /* keep this tab's choice */ }
  } else {
    saving = request;
    try {
      const result = await api.put<WebradioStatus>("/webradios/preference", { enabled });
      if (revision !== request) return;
      status = result;
    } finally { if (saving === request) saving = undefined; }
  }
  if (!allowed()) stopBroadcasts();
  notify();
}

function instanceChanged(enabled: boolean) {
  revision++;
  pending = null;
  if (status) status = { ...status, instance_enabled: enabled, enabled: enabled && status.user_enabled };
  if (!enabled) stopBroadcasts();
  notify();
  if (member !== undefined) void refresh(member);
}

if (typeof window !== "undefined") {
  window.addEventListener("soundtime:webradio-denied", () => {
    if (member !== undefined) void refresh(member);
  });
  window.addEventListener("storage", event => {
    if ((event.key === guestKey || event.key === changeKey) && member !== undefined) {
      if (member === null && !readGuest()) stopBroadcasts();
      revision++;
      pending = null;
      void refresh(member);
    }
  });
  window.addEventListener("soundtime:logout", () => {
    revision++;
    pending = null;
    member = undefined;
    status = null;
    stopBroadcasts();
  });
}

export const webPreferences = {
  get status() { return status; },
  get enabled() { return allowed(); },
  get showInNavigation() { return status ? allowed() : member !== null || guestEnabled; },
  get userEnabled() { return member == null ? guestEnabled : (status?.user_enabled ?? true); },
  get failed() { return failed; },
  refresh,
  setEnabled,
  instanceChanged,
  verifyPlayback,
};
