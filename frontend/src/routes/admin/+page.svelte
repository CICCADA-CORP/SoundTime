<script lang="ts">
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { tick, untrack } from "svelte";
  import { webPreferences } from "$lib/webradios/preferences.svelte";
  import RecordingChoice from "$lib/components/admin/RecordingChoice.svelte";
  import StorageSection from "$lib/components/admin/StorageSection.svelte";
  import { createStorageSection } from "$lib/components/admin/storage.svelte";
  import { api, pluginApi, themeApi } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import {
    LayoutDashboard, Settings, Users, Database, Sparkles,
    Radio, Globe, Network, Ban, Flag, FileText,
    HardDrive, ShieldCheck, FolderSync, Music, Share2, ScrollText, RefreshCw, Puzzle, Palette
  } from "lucide-svelte";
  import NetworkGraph from "$lib/components/NetworkGraph.svelte";
  import type {
    AdminStats,
    InstanceSetting,
    BlockedDomain,
    KnownInstance,
    P2pStatus,
    P2pPeer,
    NetworkGraph as NetworkGraphData,
    User,
    MetadataStatus,
    MetadataResult,
    MetadataTaskStatus,
    MetadataTaskProgress,
    MetadataTaskResult,
    RemoteTrack,
    HealthCheckResult,
    EditorialStatus,
    EditorialGenerateResult,
    TrackReport,
    ReportStats,
    AdminTrack,
    TosResponse,
    PaginatedResponse,
    P2pLogEntry,
    P2pLogResponse,
    ListingStatus,
    LibrarySyncOverview,
    LibrarySyncTaskStatus,
    Plugin,
    PluginConfig,
    PluginEventLog,
    Theme,
  } from "$lib/types";

  const auth = getAuthStore();
  const storage = createStorageSection();

  let activeTab = $state<string>("overview");
  let stats = $state<AdminStats | null>(null);
  let settings = $state<InstanceSetting[]>([]);
  let blockedDomains = $state<BlockedDomain[]>([]);
  let instances = $state<KnownInstance[]>([]);
  let users = $state<User[]>([]);
  let metadataStatus = $state<MetadataStatus | null>(null);
  let metadataResults = $state<(MetadataResult & { sourceTitle: string })[]>([]);
  let metadataSearch = $state("");
  let matchingTracks = $state<AdminTrack[] | null>(null);
  let findingTracks = $state(false);
  let reviewing = $state(false);
  let remoteTracks = $state<RemoteTrack[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let enriching = $state(false);
  let metadataTaskProgress = $state<MetadataTaskProgress | null>(null);
  let metadataTaskResult = $state<MetadataTaskResult | null>(null);
  let healthChecking = $state(false);
  let p2pStatus = $state<P2pStatus | null>(null);
  let p2pPeers = $state<P2pPeer[]>([]);
  let networkGraphData = $state<NetworkGraphData>({ nodes: [], links: [] });
  let addPeerInput = $state("");
  let followNewPeer = $state(true);
  let changingPeer = $state(false);
  let peerNotice = $state("");
  let p2pLogs = $state<P2pLogEntry[]>([]);
  let p2pLogsTotalInBuffer = $state(0);
  let p2pLogsAutoRefresh = $state(false);
  let p2pLogsLevelFilter = $state<string>("");
  let p2pLogsInterval: ReturnType<typeof setInterval> | null = $state(null);
  let editorialStatus = $state<EditorialStatus | null>(null);
  let editorialGenerating = $state(false);
  let editorialApiKey = $state("");
  let editorialBaseUrl = $state("https://api.openai.com/v1");
  let editorialModel = $state("gpt-4o-mini");
  let editorialMaxTracks = $state("500");
  let editorialInstanceLanguage = $state("English");

  // Lyrics state
  let lyricsProvider = $state("none");
  let lyricsMusixmatchKey = $state("");
  let lyricsLyricscomKey = $state("");
  let lyricsSaving = $state(false);

  // MusicBrainz config
  let mbBaseUrl = $state("https://musicbrainz.org/ws/2");
  let mbUserAgent = $state("SoundTime/0.1.0 (https://github.com/soundtime)");
  let mbSaving = $state(false);

  // Reports state
  let reports = $state<TrackReport[]>([]);
  let reportStats = $state<ReportStats | null>(null);
  let reportFilter = $state("all");
  let adminTracks = $state<AdminTrack[]>([]);
  let adminTrackPage = $state(1);
  let adminTrackTotal = $state(0);
  let adminTrackTotalPages = $state(0);
  let adminTrackSearch = $state("");
  let moderating = $state(false);

  // ToS state
  let tosContent = $state("");
  let tosIsDefault = $state(true);
  let tosSaving = $state(false);
  let tosSuccess = $state("");

  // Listing status
  let listingStatus = $state<ListingStatus | null>(null);
  let listingTriggering = $state(false);
  let listingTriggerError = $state<string | null>(null);

  // Library sync state
  let librarySyncOverview = $state<LibrarySyncOverview | null>(null);
  let librarySyncTask = $state<LibrarySyncTaskStatus | null>(null);
  let librarySyncPolling = $state(false);
  let librarySyncResyncingPeer = $state<string | null>(null);

  // Plugin state
  let plugins = $state<Plugin[]>([]);
  let pluginInstallUrl = $state("");
  let pluginInstalling = $state(false);
  let pluginUpdating = $state<string | null>(null);
  let selectedPlugin = $state<Plugin | null>(null);
  let pluginConfig = $state<PluginConfig[]>([]);
  let pluginLogs = $state<PluginEventLog[]>([]);
  let pluginLogsTotal = $state(0);
  let pluginLogsPage = $state(1);
  let pluginConfigNewKey = $state("");
  let pluginConfigNewValue = $state("");
  let pluginSuccess = $state("");

  // Theme state
  let themes = $state<Theme[]>([]);
  let themeInstallUrl = $state("");
  let themeInstalling = $state(false);
  let themeUpdating = $state<string | null>(null);
  let themeSuccess = $state("");

  // Block domain form
  let blockDomainInput = $state("");
  let blockReasonInput = $state("");

  /** Poll /admin/metadata/task-status until the background enrichment finishes. */
  async function pollMetadataTaskStatus() {
    const POLL_INTERVAL = 2000; // 2s (enrichment is slower than sync)
    while (true) {
      await new Promise((r) => setTimeout(r, POLL_INTERVAL));
      try {
        const status = await api.get<MetadataTaskStatus>("/admin/metadata/task-status");
        if (status.status === "running") {
          metadataTaskProgress = status.progress ?? null;
          continue;
        }
        if (status.status === "completed") {
          metadataTaskProgress = null;
          if (status.result) {
            metadataTaskResult = status.result;
          }
          // Reload metadata status to reflect new enrichment counts
          metadataStatus = await api.get<MetadataStatus>("/admin/metadata/status");
          return;
        }
        if (status.status === "error") {
          metadataTaskProgress = null;
          error = status.message ?? t("admin.metadata.taskFailed");
          return;
        }
        // idle or unknown → done
        metadataTaskProgress = null;
        return;
      } catch {
        metadataTaskProgress = null;
        return;
      }
    }
  }

  /** Poll /admin/p2p/library-sync/task-status until the background sync finishes. */
  async function pollLibrarySyncTask() {
    if (librarySyncPolling) return;
    librarySyncPolling = true;
    const POLL_INTERVAL = 2000;
    while (true) {
      await new Promise((r) => setTimeout(r, POLL_INTERVAL));
      try {
        const status = await api.get<LibrarySyncTaskStatus>("/admin/p2p/library-sync/task-status");
        librarySyncTask = status;
        if (status.status === "running") {
          continue;
        }
        // completed, error, or idle — stop polling & refresh overview
        if (activeTab === "library-sync") {
          librarySyncOverview = await api.get<LibrarySyncOverview>("/admin/p2p/library-sync");
        }
        break;
      } catch {
        break;
      }
    }
    librarySyncPolling = false;
  }

  const tabGroups = $derived([
    {
      label: t("admin.group.general"),
      tabs: [
        { id: "overview", label: t("admin.tab.overview"), icon: LayoutDashboard },
        { id: "settings", label: t("admin.tab.settings"), icon: Settings },
        { id: "users", label: t("admin.tab.users"), icon: Users },
        { id: "plugins", label: t("admin.tab.plugins"), icon: Puzzle },
        { id: "themes", label: t("admin.tab.themes"), icon: Palette },
      ],
    },
    {
      label: t("admin.group.music"),
      tabs: [
        { id: "metadata", label: t("admin.tab.metadata"), icon: Database },
        { id: "editorial", label: t("admin.tab.editorial"), icon: Sparkles },
        { id: "lyrics", label: t("admin.tab.lyrics"), icon: Music },
      ],
    },
    {
      label: t("admin.group.storage"),
      tabs: [
        { id: "storage", label: t("admin.tab.storage"), icon: HardDrive },
        { id: "integrity", label: t("admin.tab.integrity"), icon: ShieldCheck },
        { id: "sync", label: t("admin.tab.sync"), icon: FolderSync },
      ],
    },
    {
      label: t("admin.group.federation"),
      tabs: [
        { id: "p2p-status", label: t("admin.tab.p2pStatus"), icon: Network },
        { id: "network-graph", label: t("admin.tab.networkGraph"), icon: Share2 },
        { id: "p2p-logs", label: t("admin.tab.p2pLogs"), icon: ScrollText },
        { id: "library-sync", label: t("admin.tab.librarySync"), icon: RefreshCw },
        { id: "remote-tracks", label: t("admin.tab.remoteTracks"), icon: Radio },
        { id: "instances", label: t("admin.tab.instances"), icon: Globe },
        { id: "blocked", label: t("admin.tab.blocked"), icon: Ban },
      ],
    },
    {
      label: t("admin.group.moderation"),
      tabs: [
        { id: "reports", label: t("admin.tab.reports"), icon: Flag },
        { id: "tos", label: t("admin.tab.tos"), icon: FileText },
      ],
    },
  ]);

  let tabRequest = 0;
  async function loadData() {
    const request = ++tabRequest;
    loading = true;
    error = null;
    try {
      switch (activeTab) {
        case "overview":
          stats = await api.get<AdminStats>("/admin/stats");
          break;
        case "metadata":
          metadataStatus = await api.get<MetadataStatus>("/admin/metadata/status");
          // Load MusicBrainz config from settings
          const mbSettings = await api.get<InstanceSetting[]>("/admin/settings");
          const mbUrlSetting = mbSettings.find(s => s.key === "musicbrainz_base_url");
          const mbUaSetting = mbSettings.find(s => s.key === "musicbrainz_user_agent");
          if (mbUrlSetting && mbUrlSetting.value) mbBaseUrl = mbUrlSetting.value;
          if (mbUaSetting && mbUaSetting.value) mbUserAgent = mbUaSetting.value;
          // Check if enrichment task is already running
          try {
            const taskStatus = await api.get<MetadataTaskStatus>("/admin/metadata/task-status");
            if (taskStatus.status === "running") {
              enriching = true;
              metadataTaskProgress = taskStatus.progress ?? null;
              pollMetadataTaskStatus().finally(() => { enriching = false; metadataTaskProgress = null; });
            } else if (taskStatus.status === "completed" && taskStatus.result) {
              metadataTaskResult = taskStatus.result;
            } else if (taskStatus.status === "error") {
              error = taskStatus.message ?? t("admin.metadata.taskFailed");
            }
          } catch (e: unknown) {
            console.warn("Failed to check metadata task status:", e);
          }
          break;
        case "remote-tracks":
          remoteTracks = await api.get<RemoteTrack[]>("/admin/remote-tracks");
          break;
        case "settings":
          settings = await api.get<InstanceSetting[]>("/admin/settings");
          listingStatus = await api.get<ListingStatus>("/admin/listing/status").catch(() => null);
          break;
        case "blocked":
          blockedDomains = await api.get<BlockedDomain[]>("/admin/blocked-domains");
          break;
        case "instances":
          instances = await api.get<KnownInstance[]>("/admin/instances");
          break;
        case "p2p-status":
          p2pStatus = await api.get<P2pStatus>("/p2p/status");
          p2pPeers = await api.get<P2pPeer[]>("/admin/p2p/peers");
          break;
        case "network-graph":
          networkGraphData = await api.get<NetworkGraphData>("/admin/p2p/network-graph");
          break;
        case "p2p-logs": {
          const levelParam = p2pLogsLevelFilter ? `?level=${p2pLogsLevelFilter}` : "";
          const logResp = await api.get<P2pLogResponse>(`/admin/p2p/logs${levelParam}`);
          p2pLogs = logResp.entries;
          p2pLogsTotalInBuffer = logResp.total_in_buffer;
          break;
        }
        case "library-sync":
          librarySyncOverview = await api.get<LibrarySyncOverview>("/admin/p2p/library-sync");
          librarySyncTask = await api.get<LibrarySyncTaskStatus>("/admin/p2p/library-sync/task-status");
          if (librarySyncTask?.status === "running") {
            pollLibrarySyncTask();
          }
          break;
        case "users":
          users = await api.get<User[]>("/admin/users");
          break;
        case "editorial":
          editorialStatus = await api.get<EditorialStatus>("/admin/editorial/status");
          // Pre-fill fields from current settings
          const allSettings = await api.get<InstanceSetting[]>("/admin/settings");
          const keySetting = allSettings.find(s => s.key === "ai_api_key");
          const urlSetting = allSettings.find(s => s.key === "ai_base_url");
          const modelSetting = allSettings.find(s => s.key === "ai_model");
          if (keySetting) editorialApiKey = keySetting.value;
          if (urlSetting) editorialBaseUrl = urlSetting.value;
          if (modelSetting) editorialModel = modelSetting.value;
          const maxTracksSetting = allSettings.find(s => s.key === "ai_max_tracks");
          const langSetting = allSettings.find(s => s.key === "instance_language");
          if (maxTracksSetting) editorialMaxTracks = maxTracksSetting.value;
          if (langSetting) editorialInstanceLanguage = langSetting.value;
          break;
        case "lyrics":
          const lyricsSettings = await api.get<InstanceSetting[]>("/admin/settings");
          const provSetting = lyricsSettings.find(s => s.key === "lyrics_provider");
          const mmSetting = lyricsSettings.find(s => s.key === "lyrics_musixmatch_key");
          const lcSetting = lyricsSettings.find(s => s.key === "lyrics_lyricscom_key");
          if (provSetting) lyricsProvider = provSetting.value || "none";
          if (mmSetting) lyricsMusixmatchKey = mmSetting.value;
          if (lcSetting) lyricsLyricscomKey = lcSetting.value;
          break;
        case "reports":
          reports = await api.get<TrackReport[]>("/admin/reports");
          reportStats = await api.get<ReportStats>("/admin/reports/stats");
          break;
        case "tos":
          const tosData = await api.get<TosResponse>("/tos");
          tosContent = tosData.content;
          tosIsDefault = tosData.is_default;
          tosSuccess = "";
          break;
        case "storage":
        case "sync":
        case "integrity":
          await storage.load(activeTab);
          break;
        case "plugins":
          try {
            const pluginResp = await pluginApi.list();
            plugins = pluginResp.plugins;
          } catch {
            plugins = [];
          }
          break;
        case "themes":
          try {
            const themeResp = await themeApi.list();
            themes = themeResp.themes;
          } catch {
            themes = [];
          }
          break;
      }
    } catch (e: unknown) {
      if (request === tabRequest) error = (e instanceof Error ? e.message : String(e)) || t("admin.unknownError");
    } finally {
      if (request === tabRequest) loading = false;
    }
  }

  let savingRegistration = $state(false);
  let savingWebradios = $state(false);
  const registrationEnabled = $derived((settings.find(setting => setting.key === "open_registrations")?.value ?? "true") === "true");
  const privateInstance = $derived(settings.find(setting => setting.key === "instance_private")?.value === "true");

  async function updateSetting(key: string, value: string) {
    try {
      await api.put(`/admin/settings/${key}`, { value });
      if (key === "webradios_enabled") webPreferences.instanceChanged(value === "true");
      const exists = settings.some((s) => s.key === key);
      if (exists) {
        settings = settings.map((s) => (s.key === key ? { ...s, value } : s));
      } else {
        settings = [...settings, { key, value }];
      }
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function changePeer(action: () => Promise<{ message?: string }>) {
    if (changingPeer) return;
    changingPeer = true;
    error = null;
    peerNotice = "";
    try {
      const reply = await action();
      peerNotice = reply?.message || "";
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
    // Follow state is committed before a remote acknowledgement; even a failed
    // notification may have changed the effective local subscription.
    try {
      p2pPeers = await api.get<P2pPeer[]>("/admin/p2p/peers");
    } catch (cause) {
      error ||= cause instanceof Error ? cause.message : String(cause);
    } finally { changingPeer = false; }
  }

  function addPeer() {
    const nodeId = addPeerInput.trim();
    if (!nodeId) return;
    void changePeer(async () => {
      const payload = p2pStatus?.explicit_catalog ? { node_id: nodeId, follow_catalog: followNewPeer } : { node_id: nodeId };
      const response = await api.post<{ message?: string }>("/admin/p2p/peers", payload);
      addPeerInput = "";
      followNewPeer = true;
      return response;
    });
  }

  async function blockDomain() {
    if (!blockDomainInput.trim()) return;
    try {
      const result = await api.post<BlockedDomain>("/admin/blocked-domains", {
        domain: blockDomainInput.trim(),
        reason: blockReasonInput.trim() || null,
      });
      blockedDomains = [result, ...blockedDomains];
      blockDomainInput = "";
      blockReasonInput = "";
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function unblockDomain(id: string) {
    try {
      await api.delete(`/admin/blocked-domains/${id}`);
      blockedDomains = blockedDomains.filter((d) => d.id !== id);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function exportBlocklist() {
    try {
      const data = await api.get<BlockedDomain[]>("/admin/blocked-domains/export");
      const json = JSON.stringify(data.map(d => ({ domain: d.domain, reason: d.reason })), null, 2);
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `blocklist-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  let importFileInput = $state<HTMLInputElement | null>(null);

  async function importBlocklist() {
    const file = importFileInput?.files?.[0];
    if (!file) return;
    try {
      const text = await file.text();
      const entries = JSON.parse(text);
      const result = await api.post<{ imported: number; skipped: number }>("/admin/blocked-domains/import", entries);
      blockedDomains = await api.get<BlockedDomain[]>("/admin/blocked-domains");
      error = "";
      alert(t("admin.blocked.importDone", { imported: result.imported, skipped: result.skipped }));
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) ?? t("admin.importError");
    }
    if (importFileInput) importFileInput.value = "";
  }

  async function updateUserRole(userId: string, role: string) {
    try {
      await api.put(`/admin/users/${userId}/role`, { role });
      users = users.map((u) => (u.id === userId ? { ...u, role } : u));
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function banUser(userId: string) {
    const reason = prompt(t("admin.users.banReason"));
    if (reason === null) return; // User cancelled
    try {
      await api.put(`/admin/users/${userId}/ban`, { reason: reason || null });
      users = users.map((u) =>
        u.id === userId ? { ...u, is_banned: true, ban_reason: reason || null, banned_at: new Date().toISOString() } : u
      );
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function unbanUser(userId: string) {
    if (!confirm(t("admin.users.unbanConfirm"))) return;
    try {
      await api.delete(`/admin/users/${userId}/ban`);
      users = users.map((u) =>
        u.id === userId ? { ...u, is_banned: false, ban_reason: null, banned_at: null } : u
      );
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function enrichAllMetadata() {
    enriching = true;
    error = null;
    metadataTaskProgress = null;
    metadataTaskResult = null;
    try {
      await api.post("/admin/metadata/enrich-all");
      // Start polling for progress
      await pollMetadataTaskStatus();
    } catch (e: unknown) {
      // The POST may have returned 409 (already running) or other error
      // Check if a task is actually running
      try {
        const status = await api.get<MetadataTaskStatus>("/admin/metadata/task-status");
        if (status.status === "running") {
          metadataTaskProgress = status.progress ?? null;
          await pollMetadataTaskStatus();
          return;
        }
      } catch { /* truly failed */ }
      error = e instanceof Error ? e.message : String(e);
    } finally {
      enriching = false;
      metadataTaskProgress = null;
    }
  }

  async function findMetadataTracks(event: SubmitEvent) {
    event.preventDefault();
    const search = metadataSearch.trim();
    if (!search || findingTracks) return;
    findingTracks = true;
    matchingTracks = null;
    error = null;
    try {
      const params = new URLSearchParams({ search, per_page: "10" });
      const result = await api.get<PaginatedResponse<AdminTrack>>(`/admin/tracks/browse?${params}`);
      if (metadataSearch.trim() === search) matchingTracks = result.data;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      findingTracks = false;
    }
  }

  async function enrichSingleTrack(trackId: string, sourceTitle: string, recordingId?: string) {
    if (reviewing) return;
    reviewing = true;
    error = null;
    try {
      const selection = recordingId ? `?recording_id=${encodeURIComponent(recordingId)}` : "";
      const result = await api.post<MetadataResult>(`/admin/metadata/enrich/${encodeURIComponent(trackId)}${selection}`);
      if (recordingId && (result.status === "error" || result.status === "not_found")) {
        error = t(result.status === "not_found" ? "admin.metadata.recordingMissing" : "admin.metadata.recordingRetry");
        return;
      }
      metadataResults = [{ ...result, sourceTitle }, ...metadataResults.filter(row => row.track_id !== trackId)];
      await tick();
      if (activeTab === "metadata") document.getElementById(`metadata-result-${trackId}`)?.focus();
      metadataStatus = await api.get<MetadataStatus>("/admin/metadata/status");
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      reviewing = false;
    }
  }

  async function runHealthCheck() {
    healthChecking = true;
    error = null;
    try {
      await api.post<HealthCheckResult>("/admin/instances/health-check");
      // Reload instances and remote tracks
      instances = await api.get<KnownInstance[]>("/admin/instances");
      if (activeTab === "remote-tracks") {
        remoteTracks = await api.get<RemoteTrack[]>("/admin/remote-tracks");
      }
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      healthChecking = false;
    }
  }

  async function saveEditorialSettings() {
    try {
      await api.put("/admin/settings/ai_api_key", { value: editorialApiKey });
      await api.put("/admin/settings/ai_base_url", { value: editorialBaseUrl || "https://api.openai.com/v1" });
      await api.put("/admin/settings/ai_model", { value: editorialModel || "gpt-4o-mini" });
      await api.put("/admin/settings/ai_max_tracks", { value: editorialMaxTracks || "500" });
      await api.put("/admin/settings/instance_language", { value: editorialInstanceLanguage || "English" });
      editorialStatus = await api.get<EditorialStatus>("/admin/editorial/status");
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function generateEditorialPlaylists() {
    editorialGenerating = true;
    error = null;
    try {
      const result = await api.post<EditorialGenerateResult>("/admin/editorial/generate");
      editorialStatus = await api.get<EditorialStatus>("/admin/editorial/status");
      alert(result.message);
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t("admin.editorial.generationError");
    } finally {
      editorialGenerating = false;
    }
  }

  async function saveLyricsSettings() {
    lyricsSaving = true;
    error = null;
    try {
      await api.put("/admin/settings/lyrics_provider", { value: lyricsProvider });
      await api.put("/admin/settings/lyrics_musixmatch_key", { value: lyricsMusixmatchKey });
      await api.put("/admin/settings/lyrics_lyricscom_key", { value: lyricsLyricscomKey });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      lyricsSaving = false;
    }
  }

  // ── Reports functions ───────────────────────────────────────────

  async function resolveReport(id: string, action: string, trackAction: string = "none") {
    const note = action === "resolved" ? prompt(t("admin.reports.adminNote")) : null;
    try {
      await api.put(`/admin/reports/${id}`, { action, track_action: trackAction, admin_note: note ?? undefined });
      reports = await api.get<TrackReport[]>("/admin/reports");
      reportStats = await api.get<ReportStats>("/admin/reports/stats");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function loadBrowseTracks() {
    try {
      const params = new URLSearchParams({ page: adminTrackPage.toString(), per_page: "30" });
      if (adminTrackSearch.trim()) params.set("search", adminTrackSearch.trim());
      const res = await api.get<PaginatedResponse<AdminTrack>>(`/admin/tracks/browse?${params}`);
      adminTracks = res.data;
      adminTrackTotal = res.total;
      adminTrackTotalPages = res.total_pages;
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function moderateTrack(trackId: string, title: string) {
    if (!confirm(t("admin.reports.moderateConfirm", { title }))) return;
    moderating = true;
    try {
      await api.delete(`/admin/tracks/${trackId}/moderate`);
      adminTracks = adminTracks.filter(t => t.id !== trackId);
      // Refresh reports stats
      reports = await api.get<TrackReport[]>("/admin/reports");
      reportStats = await api.get<ReportStats>("/admin/reports/stats");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
    finally { moderating = false; }
  }

  function filteredReports() {
    if (reportFilter === "all") return reports;
    return reports.filter(r => r.status === reportFilter);
  }

  // ── ToS functions ───────────────────────────────────────────────

  async function saveTos() {
    tosSaving = true;
    tosSuccess = "";
    try {
      await api.put("/admin/tos", { content: tosContent });
      tosIsDefault = false;
      tosSuccess = t("admin.tos.saved");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
    finally { tosSaving = false; }
  }

  async function resetTos() {
    if (!confirm(t("admin.tos.resetConfirm"))) return;
    tosSaving = true;
    try {
      await api.delete("/admin/tos");
      const tosData = await api.get<TosResponse>("/tos");
      tosContent = tosData.content;
      tosIsDefault = true;
      tosSuccess = t("admin.tos.reset");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
    finally { tosSaving = false; }
  }

  // ── Plugin functions ───────────────────────────────────────────

  async function installPlugin() {
    if (!pluginInstallUrl.trim()) return;
    pluginInstalling = true;
    pluginSuccess = "";
    error = null;
    try {
      const newPlugin = await pluginApi.install(pluginInstallUrl.trim());
      plugins = [newPlugin, ...plugins];
      pluginInstallUrl = "";
      pluginSuccess = t("admin.plugins.installSuccess");
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t("admin.plugins.installError");
    } finally {
      pluginInstalling = false;
    }
  }

  async function enablePlugin(id: string) {
    try {
      await pluginApi.enable(id);
      plugins = plugins.map(p => p.id === id ? { ...p, status: "enabled" as const } : p);
      pluginSuccess = t("admin.plugins.enableSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function disablePlugin(id: string) {
    if (!confirm(t("admin.plugins.confirmDisable"))) return;
    try {
      await pluginApi.disable(id);
      plugins = plugins.map(p => p.id === id ? { ...p, status: "disabled" as const } : p);
      pluginSuccess = t("admin.plugins.disableSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function uninstallPlugin(id: string) {
    if (!confirm(t("admin.plugins.confirmUninstall"))) return;
    try {
      await pluginApi.uninstall(id);
      plugins = plugins.filter(p => p.id !== id);
      if (selectedPlugin?.id === id) selectedPlugin = null;
      pluginSuccess = t("admin.plugins.uninstallSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function updatePlugin(id: string) {
    pluginUpdating = id;
    try {
      const updated = await pluginApi.update(id);
      plugins = plugins.map(p => p.id === id ? updated : p);
      if (selectedPlugin?.id === id) selectedPlugin = updated;
      pluginSuccess = t("admin.plugins.updateSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
    finally { pluginUpdating = null; }
  }

  async function loadPluginConfig(plugin: Plugin) {
    selectedPlugin = plugin;
    pluginConfigNewKey = "";
    pluginConfigNewValue = "";
    try {
      const resp = await pluginApi.getConfig(plugin.id);
      pluginConfig = resp.config;
    } catch { pluginConfig = []; }
    try {
      const logResp = await pluginApi.getLogs(plugin.id, 1, 20);
      pluginLogs = logResp.logs;
      pluginLogsTotal = logResp.total;
      pluginLogsPage = 1;
    } catch { pluginLogs = []; pluginLogsTotal = 0; }
  }

  async function savePluginConfig() {
    if (!selectedPlugin) return;
    try {
      await pluginApi.updateConfig(selectedPlugin.id, pluginConfig);
      pluginSuccess = t("admin.plugins.configSaved");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  function addPluginConfigEntry() {
    if (!pluginConfigNewKey.trim()) return;
    pluginConfig = [...pluginConfig, { key: pluginConfigNewKey.trim(), value: pluginConfigNewValue }];
    pluginConfigNewKey = "";
    pluginConfigNewValue = "";
  }

  function removePluginConfigEntry(key: string) {
    pluginConfig = pluginConfig.filter(c => c.key !== key);
  }

  async function loadPluginLogs(page: number) {
    if (!selectedPlugin) return;
    try {
      const resp = await pluginApi.getLogs(selectedPlugin.id, page, 20);
      pluginLogs = resp.logs;
      pluginLogsTotal = resp.total;
      pluginLogsPage = page;
    } catch { /* silent */ }
  }

  // ── Theme functions ─────────────────────────────────────────────

  async function installTheme() {
    if (!themeInstallUrl.trim()) return;
    themeInstalling = true;
    themeSuccess = "";
    error = null;
    try {
      const newTheme = await themeApi.install(themeInstallUrl.trim());
      themes = [newTheme, ...themes];
      themeInstallUrl = "";
      themeSuccess = t("admin.themes.installSuccess");
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t("admin.themes.installError");
    } finally {
      themeInstalling = false;
    }
  }

  async function enableTheme(id: string) {
    try {
      const updated = await themeApi.enable(id);
      themes = themes.map(th => th.id === id ? updated : { ...th, status: "disabled" as const });
      themeSuccess = t("admin.themes.enableSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function disableTheme(id: string) {
    if (!confirm(t("admin.themes.confirmDisable"))) return;
    try {
      const updated = await themeApi.disable(id);
      themes = themes.map(th => th.id === id ? updated : th);
      themeSuccess = t("admin.themes.disableSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function uninstallTheme(id: string) {
    if (!confirm(t("admin.themes.confirmUninstall"))) return;
    try {
      await themeApi.uninstall(id);
      themes = themes.filter(th => th.id !== id);
      themeSuccess = t("admin.themes.uninstallSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
  }

  async function updateTheme(id: string) {
    themeUpdating = id;
    try {
      const updated = await themeApi.update(id);
      themes = themes.map(th => th.id === id ? updated : th);
      themeSuccess = t("admin.themes.updateSuccess");
    } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
    finally { themeUpdating = null; }
  }

  function switchTab(tab: string) {
    activeTab = tab;
    loadData();
  }

  $effect(() => {
    if (!auth.loading && !auth.isAdmin) {
      import("$app/navigation").then(({ goto }) => goto("/"));
    }
    if (auth.isAdmin) untrack(loadData);
  });
</script>

<svelte:head><title>{t('admin.title')} — SoundTime</title></svelte:head>

<div class="admin space-y-6">
  <CatalogHeader title={t("admin.title")} />

  {#if !auth.isAdmin}
    <div class="text-center py-16">
      <p class="text-[hsl(var(--muted-foreground))]">{t('admin.accessDenied')}</p>
    </div>
  {:else}
    <div class="admin-layout">
      <select aria-label={t("admin.title")} value={activeTab} onchange={event => switchTab(event.currentTarget.value)}
        class="admin-select h-10 w-full rounded-lg border border-input bg-background px-3 text-sm">
        {#each tabGroups as group (group.label)}
          <optgroup label={group.label}>
            {#each group.tabs as tab (tab.id)}<option value={tab.id}>{tab.label}</option>{/each}
          </optgroup>
        {/each}
      </select>
      <!-- Sidebar Navigation -->
      <aside class="admin-navigation space-y-4">
        {#each tabGroups as group (group.label)}
          <div>
            <p class="mb-1 px-2 text-xs font-medium text-muted-foreground">{group.label}</p>
            <nav class="space-y-0.5">
              {#each group.tabs as tab (tab.id)}
                <button
                  aria-current={activeTab === tab.id ? "page" : undefined}
                  class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-sm transition-colors
                    {activeTab === tab.id
                      ? 'bg-accent text-accent-foreground font-semibold'
                      : 'text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))] hover:bg-[hsl(var(--secondary))]'}"
                  onclick={() => switchTab(tab.id)}
                >
                  <tab.icon size={16} />
                  {tab.label}
                </button>
              {/each}
            </nav>
          </div>
        {/each}
      </aside>

      <!-- Main Content -->
      <div class="admin-content min-w-0 space-y-6">
        <!-- Error -->
        {#if error}
          <div role="alert" class="bg-red-500/10 border border-red-500/30 rounded-lg p-4 text-destructive">
            {error}
            <button class="ml-2 underline" onclick={() => (error = null)}>{t('common.close')}</button>
          </div>
        {/if}

        <!-- Loading -->
        {#if loading}
          <div class="text-center py-8 text-[hsl(var(--muted-foreground))]">{t('common.loading')}</div>
        {:else}

      <!-- Overview -->
      {#if activeTab === "overview" && stats}
        <div class="admin-metrics">
          <div class="admin-panel">
            <p class="text-xs font-medium text-muted-foreground">{t('admin.overview.users')}</p>
            <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{stats.total_users}</p>
          </div>
          <div class="admin-panel">
            <p class="text-xs font-medium text-muted-foreground">{t('admin.overview.tracks')}</p>
            <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{stats.total_tracks}</p>
          </div>
          <div class="admin-panel">
            <p class="text-xs font-medium text-muted-foreground">{t('admin.overview.remoteTracks')}</p>
            <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-primary">{stats.total_remote_tracks}</p>
          </div>
          <div class="admin-panel">
            <p class="text-xs font-medium text-muted-foreground">{t('admin.overview.blockedDomains')}</p>
            <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{stats.total_blocked_domains}</p>
          </div>
          <div class="admin-panel">
            <p class="text-xs font-medium text-muted-foreground">{t('admin.overview.p2pEnabled')}</p>
            <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight {stats.p2p_enabled ? 'text-[var(--admin-success)]' : 'text-[hsl(var(--muted-foreground))]'}">{stats.p2p_enabled ? '✓' : '✗'}</p>
          </div>
          {#if stats.p2p_node_id}
            <div class="admin-panel sm:col-span-2">
              <p class="text-xs font-medium text-muted-foreground">{t('admin.overview.p2pNodeId')}</p>
              <p class="text-sm font-mono mt-1 truncate">{stats.p2p_node_id}</p>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Metadata Enrichment -->
      {#if activeTab === "metadata"}
        <div class="space-y-6">
          {#if metadataStatus}
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.totalTracks')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{metadataStatus.total_tracks}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.enrichedMB')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-[var(--admin-success)]">{metadataStatus.enriched_tracks}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.pending')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight {metadataStatus.pending_tracks > 0 ? 'text-[var(--admin-warning)]' : ''}">{metadataStatus.pending_tracks}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.withBitrate')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{metadataStatus.tracks_with_bitrate}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.albumsWithCover')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{metadataStatus.albums_with_cover} / {metadataStatus.total_albums}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.federatedTracks')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{metadataStatus.total_remote_tracks}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.metadata.availableInstances')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-[var(--admin-success)]">{metadataStatus.available_remote_tracks}</p>
              </div>
            </div>
          {/if}

          <!-- Enrichment Progress Bar -->
          {#if metadataStatus && metadataStatus.total_tracks > 0}
            <div class="admin-panel">
              <div class="flex items-center justify-between mb-2">
                <p class="text-sm font-medium">{t('admin.metadata.enrichmentProgress')}</p>
                <p class="text-sm text-[hsl(var(--muted-foreground))]">
                  {Math.round((metadataStatus.enriched_tracks / metadataStatus.total_tracks) * 100)}%
                </p>
              </div>
              <div class="w-full bg-[hsl(var(--secondary))] rounded-full h-2">
                <div
                  class="bg-[hsl(var(--primary))] h-2 rounded-full transition-all"
                  style="width: {(metadataStatus.enriched_tracks / metadataStatus.total_tracks) * 100}%"
                ></div>
              </div>
            </div>
          {/if}

          <!-- MusicBrainz Configuration -->
          <div class="admin-panel space-y-4">
            <div>
              <h3 class="font-medium">{t('admin.metadata.mbConfig')}</h3>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{t('admin.metadata.mbConfigDesc')}</p>
            </div>
            <div class="space-y-3">
              <div>
                <label for="mb-base-url" class="text-sm block mb-1">{t('admin.metadata.mbBaseUrl')}</label>
                <input id="mb-base-url" type="url" bind:value={mbBaseUrl} placeholder="https://musicbrainz.org/ws/2" class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm font-mono border border-[hsl(var(--border))]" />
              </div>
              <div>
                <label for="mb-user-agent" class="text-sm block mb-1">{t('admin.metadata.mbUserAgent')}</label>
                <input id="mb-user-agent" type="text" bind:value={mbUserAgent} placeholder="SoundTime/0.1.0 (https://github.com/soundtime)" class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm font-mono border border-[hsl(var(--border))]" />
              </div>
              <div class="flex items-center gap-3">
                <button
                  class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium disabled:opacity-50"
                  disabled={mbSaving}
                  onclick={async () => {
                    mbSaving = true;
                    try {
                      await updateSetting("musicbrainz_base_url", mbBaseUrl);
                      await updateSetting("musicbrainz_user_agent", mbUserAgent);
                    } catch { /* empty */ } finally { mbSaving = false; }
                  }}
                >
                  {mbSaving ? t('common.loading') : t('common.save')}
                </button>
                <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.metadata.mbRateLimit')}</p>
              </div>
            </div>
          </div>

          <!-- Actions -->
          <div class="flex gap-4">
            <button
              class="bg-[hsl(var(--primary))] hover:opacity-90 text-primary-foreground px-6 py-3 rounded-lg text-sm font-medium transition disabled:opacity-50"
              onclick={() => enrichAllMetadata()}
              disabled={enriching}
            >
              {#if enriching}
                {t('admin.metadata.enriching')}
              {:else}
                {t('admin.metadata.enrichAll')}
              {/if}
            </button>
            <button
              class="bg-[hsl(var(--secondary))] hover:opacity-90 text-[hsl(var(--foreground))] px-6 py-3 rounded-lg text-sm font-medium transition disabled:opacity-50"
              onclick={() => runHealthCheck()}
              disabled={healthChecking}
            >
              {#if healthChecking}
                {t('admin.metadata.checking')}
              {:else}
                {t('admin.metadata.checkAvailability')}
              {/if}
            </button>
          </div>

          <!-- Enrichment Progress (background task) -->
          {#if enriching && metadataTaskProgress}
            <div class="admin-panel">
              <div class="flex items-center justify-between text-sm text-[hsl(var(--muted-foreground))] mb-2">
                <span class="flex items-center gap-2">
                  <div class="w-4 h-4 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
                  {t('admin.metadata.enriching')}
                  {#if metadataTaskProgress.current_track}
                    — {metadataTaskProgress.current_track}
                  {/if}
                </span>
                <span>{metadataTaskProgress.processed} / {metadataTaskProgress.total}</span>
              </div>
              <div class="w-full bg-[hsl(var(--secondary))] rounded-full h-2 mb-3">
                <div
                  class="bg-[hsl(var(--primary))] h-2 rounded-full transition-all duration-300"
                  style="width: {metadataTaskProgress.total > 0 ? (metadataTaskProgress.processed / metadataTaskProgress.total * 100) : 0}%"
                ></div>
              </div>
              <div class="flex flex-wrap gap-4 text-xs text-[hsl(var(--muted-foreground))]">
                <span class="text-[var(--admin-success)]">{t('admin.metadata.statusEnrichedMB')}: {metadataTaskProgress.enriched}</span>
                <span>{t('admin.metadata.statusAmbiguous')}: {metadataTaskProgress.ambiguous ?? 0}</span>
                <span class="text-[var(--admin-warning)]">{t('admin.metadata.statusNotFound')}: {metadataTaskProgress.not_found}</span>
                <span class="text-destructive">{t('admin.metadata.statusError')}: {metadataTaskProgress.errors}</span>
              </div>
            </div>
          {/if}

          <!-- Enrichment Task Result (completed background task) -->
          {#if metadataTaskResult}
            <div class="bg-[hsl(var(--card))] rounded-lg border border-green-500/30 p-5">
              <div class="flex items-center justify-between mb-3">
                <h3 class="font-medium text-[var(--admin-success)]">{t('admin.metadata.enrichmentComplete')}</h3>
                <button
                  class="text-xs text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))] transition"
                  onclick={async () => {
                    metadataTaskResult = null;
                    try { await api.post("/admin/metadata/task-dismiss"); } catch { /* ignore */ }
                  }}
                >
                  {t('common.dismiss')}
                </button>
              </div>
              <div class="admin-metrics">
                <div class="bg-[hsl(var(--secondary))] rounded-lg p-3 text-center">
                  <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.metadata.totalTracks')}</p>
                  <p class="text-xl font-bold mt-1">{metadataTaskResult.total_processed}</p>
                </div>
                <div class="bg-green-500/10 rounded-lg p-3 text-center">
                  <p class="text-xs text-[var(--admin-success)]">{t('admin.metadata.statusEnrichedMB')}</p>
                  <p class="text-xl font-bold mt-1 text-[var(--admin-success)]">{metadataTaskResult.enriched}</p>
                </div>
                <div class="bg-yellow-500/10 rounded-lg p-3 text-center">
                  <p class="text-xs text-[var(--admin-warning)]">{t('admin.metadata.statusNotFound')}</p>
                  <p class="text-xl font-bold mt-1 text-[var(--admin-warning)]">{metadataTaskResult.not_found}</p>
                </div>
                <div class="bg-[hsl(var(--secondary))] rounded-lg p-3 text-center">
                  <p class="text-xs">{t('admin.metadata.statusAmbiguous')}</p>
                  <p class="text-xl font-bold mt-1">{metadataTaskResult.ambiguous ?? 0}</p>
                </div>
                <div class="rounded-lg p-3 text-center {metadataTaskResult.errors > 0 ? 'bg-red-500/10' : 'bg-[hsl(var(--secondary))]'}">
                  <p class="text-xs {metadataTaskResult.errors > 0 ? 'text-destructive' : 'text-[hsl(var(--muted-foreground))]'}">{t('admin.metadata.statusError')}</p>
                  <p class="text-xl font-bold mt-1 {metadataTaskResult.errors > 0 ? 'text-destructive' : ''}">{metadataTaskResult.errors}</p>
                </div>
              </div>
            </div>
          {/if}

          <div class="admin-panel">
            <form onsubmit={findMetadataTracks}>
              <label for="metadata-track-search" class="block font-medium mb-2">{t("admin.metadata.findTrack")}</label>
              <div class="flex gap-2">
                <input id="metadata-track-search" type="search" required bind:value={metadataSearch} oninput={() => matchingTracks = null}
                  class="min-w-0 flex-1 rounded border border-[hsl(var(--border))] bg-[hsl(var(--secondary))] px-3 py-2 text-sm" />
                <button type="submit" disabled={findingTracks || !metadataSearch.trim()}
                  class="rounded bg-[hsl(var(--secondary))] px-3 py-2 text-sm font-medium disabled:opacity-50">
                  {t("admin.reports.searchBtn")}
                </button>
              </div>
            </form>
            {#if matchingTracks}
              {#if matchingTracks.length}
                <ul class="mt-3 space-y-1">
                  {#each matchingTracks as track (track.id)}
                    <li><button type="button" disabled={reviewing}
                      class="rounded px-2 py-1 text-left text-sm underline disabled:opacity-50"
                      onclick={() => enrichSingleTrack(track.id, track.title)}>
                      {t("admin.metadata.reviewTrack", { title: [track.title, track.artist_name].filter(Boolean).join(" · ") })}
                    </button></li>
                  {/each}
                </ul>
              {:else}
                <p role="status" class="mt-3 text-sm">{t("explore.noTracks")}</p>
              {/if}
            {/if}
          </div>

          <!-- Individual Enrichment Results (from single-track enrichments) -->
          {#if metadataResults.length > 0}
            <div class="bg-[hsl(var(--card))] rounded-lg overflow-x-auto">
              <div class="p-4 border-b border-[hsl(var(--border))]">
                <h3 class="font-medium">{t('admin.metadata.enrichmentResults', { count: metadataResults.length })}</h3>
              </div>
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b border-[hsl(var(--border))]">
                    <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.metadata.colStatus')}</th>
                    <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.metadata.colTitle')}</th>
                    <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.metadata.colArtist')}</th>
                    <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.metadata.colAlbum')}</th>
                    <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.metadata.colGenre')}</th>
                    <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.metadata.colCover')}</th>
                  </tr>
                </thead>
                <tbody>
                  {#each metadataResults as result (result.track_id)}
                    <tr class="border-b border-[hsl(var(--border))] last:border-b-0">
                      <td class="p-3">
                        <span class="px-2 py-0.5 rounded text-xs font-medium
                          {result.status === 'enriched' ? 'bg-green-500/20 text-[var(--admin-success)]' :
                           result.status === 'enriched_by_ai' ? 'bg-purple-500/20 text-[var(--admin-special)]' :
                           result.status === 'ambiguous' ? 'bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))]' :
                           result.status === 'not_found' ? 'bg-yellow-500/20 text-[var(--admin-warning)]' :
                           result.status === 'already_enriched' ? 'bg-blue-500/20 text-primary' :
                           'bg-red-500/20 text-destructive'}">
                          {result.status === 'enriched' ? t('admin.metadata.statusEnrichedMB') :
                           result.status === 'enriched_by_ai' ? t('admin.metadata.statusEnrichedAI') :
                           result.status === 'ambiguous' ? t('admin.metadata.statusAmbiguous') :
                           result.status === 'not_found' ? t('admin.metadata.statusNotFound') :
                           result.status === 'already_enriched' ? t('admin.metadata.statusAlreadyEnriched') :
                           t('admin.metadata.statusError')}
                        </span>
                      </td>
                      <th scope="row" tabindex="-1" id={`metadata-result-${result.track_id}`}
                        class="p-3 max-w-[200px] truncate text-left font-medium">{result.corrected_title ?? result.sourceTitle}</th>
                      <td class="p-3 max-w-[150px] truncate">{result.artist_name ?? "—"}</td>
                      <td class="p-3 max-w-[150px] truncate">{result.album_title ?? "—"}</td>
                      <td class="p-3">{result.genre ?? "—"}</td>
                      <td class="p-3 text-center">
                        {#if result.cover_url}
                          <img src={result.cover_url} alt="" class="w-8 h-8 rounded mx-auto object-cover" />
                        {:else}
                          <span class="text-[hsl(var(--muted-foreground))]">—</span>
                        {/if}
                      </td>
                    </tr>
                    {#if result.status === "ambiguous" && result.candidates?.length}
                      <tr><td colspan="6" class="p-3">
                        <RecordingChoice title={result.sourceTitle} candidates={result.candidates} busy={reviewing}
                          choose={(id) => enrichSingleTrack(result.track_id, result.sourceTitle, id)} />
                      </td></tr>
                    {/if}
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Library Sync -->
      {#if activeTab === "library-sync"}
        <div class="space-y-6">
          <!-- Background task status banner -->
          {#if librarySyncTask && librarySyncTask.status !== "idle"}
            <div class="rounded-lg p-4 {librarySyncTask.status === 'running' ? 'bg-blue-500/10 border border-blue-500/30' : librarySyncTask.status === 'completed' ? 'bg-green-500/10 border border-green-500/30' : 'bg-red-500/10 border border-red-500/30'}">
              {#if librarySyncTask.status === "running"}
                <div class="flex items-center justify-between mb-2">
                  <h4 class="text-sm font-semibold text-primary flex items-center gap-2">
                    <div class="w-4 h-4 border-2 border-blue-400 border-t-transparent rounded-full animate-spin"></div>
                    {t('admin.libSync.taskRunning')}
                  </h4>
                  <span class="text-xs text-[hsl(var(--muted-foreground))] font-mono">{librarySyncTask.peer_id.slice(0, 12)}…</span>
                </div>
                <p class="text-sm text-[hsl(var(--muted-foreground))] mb-2">{librarySyncTask.progress.phase}</p>
                {#if librarySyncTask.progress.total}
                  <div class="w-full h-2 bg-[hsl(var(--secondary))] rounded-full overflow-hidden">
                    <div class="h-full bg-blue-500 rounded-full transition-all duration-300"
                         style="width: {Math.min((librarySyncTask.progress.processed / librarySyncTask.progress.total) * 100, 100)}%"></div>
                  </div>
                  <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{librarySyncTask.progress.processed} / {librarySyncTask.progress.total}</p>
                {/if}
              {:else if librarySyncTask.status === "completed"}
                <div class="flex items-center justify-between">
                  <div>
                    <h4 class="text-sm font-semibold text-[var(--admin-success)]">{t('admin.libSync.taskCompleted')}</h4>
                    <p class="text-sm text-[hsl(var(--muted-foreground))]">
                      {librarySyncTask.result.tracks_synced} {t('admin.libSync.tracksSynced')} — {t('admin.libSync.duration')}: {librarySyncTask.result.duration_secs}s
                    </p>
                  </div>
                  <button
                    class="text-xs bg-[hsl(var(--secondary))] px-3 py-1.5 rounded hover:opacity-80 transition"
                    onclick={async () => {
                      await api.post("/admin/p2p/library-sync/task-dismiss");
                      librarySyncTask = { status: "idle" };
                      await loadData();
                    }}
                  >
                    {t('admin.libSync.taskDismiss')}
                  </button>
                </div>
              {:else if librarySyncTask.status === "error"}
                <div class="flex items-center justify-between">
                  <div>
                    <h4 class="text-sm font-semibold text-destructive">{t('admin.libSync.taskError')}</h4>
                    <p class="text-sm text-red-300">{librarySyncTask.message}</p>
                  </div>
                  <button
                    class="text-xs bg-[hsl(var(--secondary))] px-3 py-1.5 rounded hover:opacity-80 transition"
                    onclick={async () => {
                      await api.post("/admin/p2p/library-sync/task-dismiss");
                      librarySyncTask = { status: "idle" };
                    }}
                  >
                    {t('admin.libSync.taskDismiss')}
                  </button>
                </div>
              {/if}
            </div>
          {/if}

          <!-- Header + refresh -->
          <div class="flex items-center justify-between">
            <h2 class="text-lg font-semibold">{t('admin.libSync.title')}</h2>
            <button
              class="text-xs bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] px-3 py-1.5 rounded hover:opacity-90 transition"
              onclick={async () => {
                await loadData();
                // If a task is running, start polling
                if (librarySyncTask?.status === "running" && !librarySyncPolling) {
                  pollLibrarySyncTask();
                }
              }}
            >
              {t('admin.libSync.refresh')}
            </button>
          </div>

          {#if librarySyncOverview}
            <!-- Overview cards -->
            <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-4">
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.libSync.localTracks')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{librarySyncOverview.local_track_count}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.libSync.totalPeers')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{librarySyncOverview.total_peers}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.libSync.syncedPeers')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-[var(--admin-success)]">{librarySyncOverview.synced_peers}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.libSync.partialPeers')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-[var(--admin-warning)]">{librarySyncOverview.partial_peers}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.libSync.notSyncedPeers')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-destructive">{librarySyncOverview.not_synced_peers}</p>
              </div>
            </div>

            <!-- Per-peer sync table -->
            {#if librarySyncOverview.peers.length > 0}
              <div class="bg-[hsl(var(--card))] rounded-lg overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b border-[hsl(var(--border))]">
                      <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.peer')}</th>
                      <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.announced')}</th>
                      <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.cataloged')}</th>
                      <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.available')}</th>
                      <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.ratio')}</th>
                      <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.state')}</th>
                      <th class="text-right p-3 text-[hsl(var(--muted-foreground))]">{t('admin.libSync.actions')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each librarySyncOverview.peers as peer}
                      <tr class="border-b border-[hsl(var(--border))] last:border-b-0">
                        <td class="p-3">
                          <div class="flex items-center gap-2">
                            <div class="w-2 h-2 rounded-full flex-shrink-0 {peer.is_online ? 'bg-green-400' : 'bg-gray-500'}"></div>
                            <div class="min-w-0">
                              <p class="font-mono text-xs truncate max-w-[220px]">
                                {peer.name ? `${peer.name}` : `${peer.node_id.slice(0, 16)}…`}
                              </p>
                              {#if peer.version}
                                <p class="text-[10px] text-[hsl(var(--muted-foreground))]">v{peer.version}</p>
                              {/if}
                            </div>
                          </div>
                        </td>
                        <td class="p-3 text-center font-mono">{peer.peer_announced_tracks}</td>
                        <td class="p-3 text-center font-mono">{peer.local_remote_tracks}</td>
                        <td class="p-3 text-center font-mono">{peer.available_tracks}</td>
                        <td class="p-3 text-center">
                          <div class="flex items-center justify-center gap-2">
                            <div class="w-16 h-1.5 bg-[hsl(var(--secondary))] rounded-full overflow-hidden">
                              <div class="h-full rounded-full transition-all {peer.sync_ratio >= 1 ? 'bg-green-400' : peer.sync_ratio > 0 ? 'bg-yellow-400' : 'bg-red-400'}"
                                   style="width: {Math.min(peer.sync_ratio * 100, 100)}%"></div>
                            </div>
                            <span class="text-xs font-mono">{Math.round(peer.sync_ratio * 100)}%</span>
                          </div>
                        </td>
                        <td class="p-3 text-center">
                          {#if peer.sync_state === "synced"}
                            <span class="text-xs px-2 py-0.5 rounded-full bg-green-500/20 text-[var(--admin-success)]">{t('admin.libSync.stateSynced')}</span>
                          {:else if peer.sync_state === "partial"}
                            <span class="text-xs px-2 py-0.5 rounded-full bg-yellow-500/20 text-[var(--admin-warning)]">{t('admin.libSync.statePartial')}</span>
                          {:else if peer.sync_state === "not_synced"}
                            <span class="text-xs px-2 py-0.5 rounded-full bg-red-500/20 text-destructive">{t('admin.libSync.stateNotSynced')}</span>
                          {:else if peer.sync_state === "offline"}
                            <span class="text-xs px-2 py-0.5 rounded-full bg-gray-500/20 text-gray-400">{t('admin.libSync.stateOffline')}</span>
                          {:else}
                            <span class="text-xs px-2 py-0.5 rounded-full bg-gray-500/20 text-gray-400">{t('admin.libSync.stateEmpty')}</span>
                          {/if}
                        </td>
                        <td class="p-3 text-right">
                          <button
                            class="text-xs px-3 py-1.5 rounded transition font-medium disabled:opacity-50 {librarySyncResyncingPeer === peer.node_id
                              ? 'bg-blue-500/20 text-primary cursor-wait'
                              : 'bg-[hsl(var(--primary))]/20 text-[hsl(var(--primary))] hover:bg-[hsl(var(--primary))]/30'}"
                            disabled={!peer.is_online || librarySyncResyncingPeer !== null || librarySyncTask?.status === "running"}
                            onclick={async () => {
                              librarySyncResyncingPeer = peer.node_id;
                              try {
                                await api.post(`/admin/p2p/library-sync/${peer.node_id}`);
                                // Start polling for task status
                                pollLibrarySyncTask();
                              } catch (e: unknown) {
                                error = e instanceof Error ? e.message : String(e);
                              } finally {
                                librarySyncResyncingPeer = null;
                              }
                            }}
                          >
                            {#if librarySyncResyncingPeer === peer.node_id}
                              {t('admin.libSync.syncing')}
                            {:else}
                              {t('admin.libSync.resync')}
                            {/if}
                          </button>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="bg-[hsl(var(--card))] rounded-lg p-8 text-center">
                <RefreshCw class="w-10 h-10 text-[hsl(var(--muted-foreground))]/30 mx-auto mb-3" />
                <p class="text-[hsl(var(--muted-foreground))]">{t('admin.libSync.noPeers')}</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-2">{t('admin.libSync.noPeersHint')}</p>
              </div>
            {/if}
          {/if}
        </div>
      {/if}

      <!-- Remote Tracks (Federated) -->
      {#if activeTab === "remote-tracks"}
        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <p class="text-sm text-[hsl(var(--muted-foreground))]">
              {t('admin.remote.description')}
            </p>
            <button
              class="bg-[hsl(var(--secondary))] hover:opacity-90 text-[hsl(var(--foreground))] px-4 py-2 rounded text-sm font-medium transition disabled:opacity-50"
              onclick={() => runHealthCheck()}
              disabled={healthChecking}
            >
              {healthChecking ? t('admin.metadata.checking') : t('admin.remote.checkAvailability')}
            </button>
          </div>

          <div class="bg-[hsl(var(--card))] rounded-lg overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-[hsl(var(--border))]">
                  <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colTitle')}</th>
                  <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colArtist')}</th>
                  <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colInstance')}</th>
                  <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colBitrate')}</th>
                  <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colFormat')}</th>
                  <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colAvailable')}</th>
                  <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colLinked')}</th>
                  <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.remote.colActions')}</th>
                </tr>
              </thead>
              <tbody>
                {#each remoteTracks as rt}
                  <tr class="border-b border-[hsl(var(--border))] last:border-b-0">
                    <td class="p-3 max-w-[200px] truncate font-medium">{rt.title}</td>
                    <td class="p-3 max-w-[150px] truncate text-[hsl(var(--muted-foreground))]">{rt.artist_name}</td>
                    <td class="p-3 font-mono text-xs">{rt.instance_domain}</td>
                    <td class="p-3 text-center font-mono text-xs">
                      {#if rt.bitrate}
                        {rt.bitrate}k
                      {:else}
                        —
                      {/if}
                    </td>
                    <td class="p-3 text-center text-xs uppercase">{rt.format ?? "—"}</td>
                    <td class="p-3 text-center">
                      {#if rt.is_available}
                        <span class="text-[var(--admin-success)] text-xs font-medium">{t('admin.remote.online')}</span>
                      {:else}
                        <span class="text-destructive text-xs font-medium">{t('admin.remote.offline')}</span>
                      {/if}
                    </td>
                    <td class="p-3 text-center">
                      {#if rt.local_track_id}
                        <span class="text-primary text-xs" title="{t('admin.remote.colLinked')}">{t('common.yes')}</span>
                      {:else}
                        <span class="text-[hsl(var(--muted-foreground))] text-xs">{t('common.no')}</span>
                      {/if}
                    </td>
                    <td class="p-3 text-center">
                      {#if rt.is_available}
                        <button
                          class="text-xs text-destructive hover:text-red-300 font-medium transition"
                          onclick={async () => {
                            try {
                              await api.patch(`/admin/p2p/tracks/${rt.id}/dereference`);
                              rt.is_available = false;
                            } catch (e) { console.error(e); }
                          }}
                        >
                          {t('admin.remote.dereference')}
                        </button>
                      {:else}
                        <button
                          class="text-xs text-[var(--admin-success)] hover:text-green-300 font-medium transition"
                          onclick={async () => {
                            try {
                              await api.patch(`/admin/p2p/tracks/${rt.id}/rereference`);
                              rt.is_available = true;
                            } catch (e) { console.error(e); }
                          }}
                        >
                          {t('admin.remote.rereference')}
                        </button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
            {#if remoteTracks.length === 0}
              <p class="p-4 text-[hsl(var(--muted-foreground))] text-sm">{t('admin.remote.noTracks')}</p>
            {/if}
          </div>
        </div>
      {/if}

      <!-- Settings -->
      {#if activeTab === "settings"}
        <div class="admin-panel mb-4">
          <label class="flex items-center justify-between gap-4">
            <span class="text-sm font-semibold">{t("webradios.instanceSetting")}</span>
            <input type="checkbox" role="switch" class="size-5 accent-primary" disabled={savingWebradios}
              checked={settings.find(entry => entry.key === "webradios_enabled")?.value !== "false"}
              onchange={async event => {
                const enabled = event.currentTarget.checked;
                event.currentTarget.checked = !enabled;
                savingWebradios = true;
                try { await updateSetting("webradios_enabled", String(enabled)); }
                finally { savingWebradios = false; }
              }} />
          </label>
        </div>
        <!-- Private Instance Toggle -->
        <div class="admin-panel mb-4">
          <div class="flex items-center justify-between">
            <div>
              <h3 class="text-sm font-semibold">{t('admin.settings.privateInstance')}</h3>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{t('admin.settings.privateInstanceDesc')}</p>
            </div>
            {#if settings.find(s => s.key === 'instance_private')?.value === 'true'}
              <button
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors bg-[hsl(var(--primary))]"
                onclick={() => updateSetting('instance_private', 'false')}
                role="switch"
                aria-checked={true}
                aria-label={t('admin.settings.privateInstance')}
              >
                <span class="inline-block h-4 w-4 rounded-full bg-white transition-transform translate-x-6"></span>
              </button>
            {:else}
              <button
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors bg-[hsl(var(--secondary))]"
                onclick={() => updateSetting('instance_private', 'true')}
                role="switch"
                aria-checked={false}
                aria-label={t('admin.settings.privateInstance')}
              >
                <span class="inline-block h-4 w-4 rounded-full bg-white transition-transform translate-x-1"></span>
              </button>
            {/if}
          </div>
          {#if settings.find(s => s.key === 'instance_private')?.value === 'true'}
            <div class="mt-3 flex items-center gap-2 text-xs text-amber-400 bg-amber-500/10 rounded-md px-3 py-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 9v4"/><path d="M12 17h.01"/><path d="M3.586 15.424 12 21.414l8.414-5.99a2 2 0 0 0 .586-1.424V4a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 .586 1.424z"/></svg>
              {t('admin.settings.privateWarning')}
            </div>
          {/if}
        </div>

        <div class="admin-panel mb-4">
          <label class="flex items-center justify-between gap-4">
            <span><strong class="block text-sm font-semibold">{t("setup.openRegistrations")}</strong>
              <span class="mt-1 block text-xs text-muted-foreground">{t("setup.openRegistrationsHint")}</span>
            </span>
            <input type="checkbox" class="size-5 accent-primary" disabled={privateInstance || savingRegistration}
              checked={!privateInstance && registrationEnabled}
              onchange={async (event) => {
                const enabled = event.currentTarget.checked;
                event.currentTarget.checked = !enabled;
                savingRegistration = true;
                try { await updateSetting("open_registrations", String(enabled)); }
                finally { savingRegistration = false; }
              }} />
          </label>
        </div>

        <!-- Public Listing Toggle -->
        <div class="admin-panel mb-4">
          <div class="flex items-center justify-between">
            <div>
              <h3 class="text-sm font-semibold">{t('admin.settings.publicListing')}</h3>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{t('admin.settings.publicListingDesc')}</p>
            </div>
            {#if (settings.find(s => s.key === 'listing_public')?.value ?? 'false') === 'true'}
              <button
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors bg-[hsl(var(--primary))]"
                onclick={async () => {
                  await updateSetting('listing_public', 'false');
                  try {
                    await api.post('/admin/listing/delist', {});
                  } catch {}
                }}
                role="switch"
                aria-checked={true}
                aria-label={t('admin.settings.publicListing')}
              >
                <span class="inline-block h-4 w-4 rounded-full bg-white transition-transform translate-x-6"></span>
              </button>
            {:else}
              <button
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors bg-[hsl(var(--secondary))]"
                onclick={async () => { await updateSetting('listing_public', 'true'); }}
                role="switch"
                aria-checked={false}
                aria-label={t('admin.settings.publicListing')}
              >
                <span class="inline-block h-4 w-4 rounded-full bg-white transition-transform translate-x-1"></span>
              </button>
            {/if}
          </div>
          {#if (settings.find(s => s.key === 'listing_public')?.value ?? 'false') === 'true'}
            <!-- Real listing status from backend -->
            {#if listingStatus}
              {#if listingStatus.domain_is_local}
                <div class="mt-3 flex items-center gap-2 text-xs text-destructive bg-red-500/10 rounded-md px-3 py-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 9v4"/><path d="M12 17h.01"/><path d="M3.586 15.424 12 21.414l8.414-5.99a2 2 0 0 0 .586-1.424V4a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 .586 1.424z"/></svg>
                  {t('admin.settings.listingDomainLocalError')}
                </div>
              {:else if listingStatus.status === 'ok'}
                <div class="mt-3 flex items-center gap-2 text-xs text-[var(--admin-success)] bg-green-500/10 rounded-md px-3 py-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/></svg>
                  <div>
                    <span>{t('admin.settings.listingStatusOk')}</span>
                    {#if listingStatus.last_heartbeat}
                      <span class="ml-2 text-[hsl(var(--muted-foreground))]">{t('admin.settings.listingLastHeartbeat')}: {new Date(listingStatus.last_heartbeat).toLocaleString()}</span>
                    {/if}
                  </div>
                </div>
              {:else if listingStatus.status === 'error'}
                <div class="mt-3 flex flex-col gap-1 text-xs text-destructive bg-red-500/10 rounded-md px-3 py-2">
                  <div class="flex items-center gap-2">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/></svg>
                    <span class="font-medium">{t('admin.settings.listingStatusError')}</span>
                  </div>
                  {#if listingStatus.error}
                    <p class="text-red-300/80 ml-6 break-words">{listingStatus.error}</p>
                  {/if}
                  {#if listingStatus.last_heartbeat}
                    <span class="ml-6 text-[hsl(var(--muted-foreground))]">{t('admin.settings.listingLastHeartbeat')}: {new Date(listingStatus.last_heartbeat).toLocaleString()}</span>
                  {/if}
                </div>
              {:else}
                <div class="mt-3 flex items-center gap-2 text-xs text-amber-400 bg-amber-500/10 rounded-md px-3 py-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>
                  {t('admin.settings.listingStatusUnknown')}
                </div>
              {/if}
            {/if}

            <!-- Trigger heartbeat button -->
            <div class="mt-3">
              <button
                class="px-3 py-1.5 rounded-md text-xs font-medium transition bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] hover:opacity-90 disabled:opacity-50"
                disabled={listingTriggering}
                onclick={async () => {
                  listingTriggering = true;
                  listingTriggerError = null;
                  try {
                    await api.post('/admin/listing/trigger', {});
                    listingStatus = await api.get<ListingStatus>('/admin/listing/status').catch(() => listingStatus);
                  } catch (e: unknown) {
                    const body = (e as { body?: { message?: string } })?.body;
                    listingTriggerError = body?.message || (e instanceof Error ? e.message : 'Unknown error');
                    listingStatus = await api.get<ListingStatus>('/admin/listing/status').catch(() => listingStatus);
                  } finally {
                    listingTriggering = false;
                  }
                }}
              >
                {listingTriggering ? t('admin.settings.listingTriggerSending') : t('admin.settings.listingTrigger')}
              </button>
              {#if listingTriggerError}
                <p class="text-xs text-destructive mt-1.5">{listingTriggerError}</p>
              {/if}
            </div>

            <!-- Listing Domain -->
            <div class="mt-3">
              <label for="listing-domain-input" class="block text-xs font-medium text-[hsl(var(--muted-foreground))] mb-1.5">{t('admin.settings.listingDomain')}</label>
              <div class="flex gap-2">
                <input
                  id="listing-domain-input"
                  type="text"
                  placeholder={t('admin.settings.listingDomainPlaceholder')}
                  value={settings.find(s => s.key === 'listing_domain')?.value || ''}
                  class="flex-1 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-md px-3 py-2 text-sm border border-[hsl(var(--border))] outline-none focus:border-[hsl(var(--primary))] transition-colors font-mono"
                  onchange={(e) => updateSetting('listing_domain', (e.target as HTMLInputElement).value.replace(/^https?:\/\//, '').replace(/\/$/, ''))}
                />
              </div>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1.5">{t('admin.settings.listingDomainDesc')}</p>
              {#if listingStatus?.domain_is_local}
                <div class="mt-2 flex items-center gap-2 text-xs text-destructive bg-red-500/10 rounded-md px-3 py-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 9v4"/><path d="M12 17h.01"/><path d="M3.586 15.424 12 21.414l8.414-5.99a2 2 0 0 0 .586-1.424V4a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 .586 1.424z"/></svg>
                  {t('admin.settings.listingDomainWarning')}
                </div>
              {/if}
              {#if listingStatus}
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1 font-mono">{listingStatus.domain}</p>
              {/if}
            </div>
            <!-- Listing URL -->
            <div class="mt-3">
              <label for="listing-url-input" class="block text-xs font-medium text-[hsl(var(--muted-foreground))] mb-1.5">{t('admin.settings.listingUrl')}</label>
              <div class="flex gap-2">
                <input
                  id="listing-url-input"
                  type="url"
                  placeholder="https://soundtime-listing-production.up.railway.app"
                  value={settings.find(s => s.key === 'listing_url')?.value || ''}
                  class="flex-1 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-md px-3 py-2 text-sm border border-[hsl(var(--border))] outline-none focus:border-[hsl(var(--primary))] transition-colors"
                  onchange={(e) => updateSetting('listing_url', (e.target as HTMLInputElement).value.replace(/\/$/, ''))}
                />
              </div>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1.5">{t('admin.settings.listingUrlDesc')}</p>
            </div>
          {/if}
        </div>

        <div class="bg-[hsl(var(--card))] rounded-lg divide-y divide-[hsl(var(--border))]">
          {#each settings.filter(setting => setting.key !== "open_registrations" && setting.key !== "webradios_enabled") as setting (setting.key)}
            <div class="p-4 flex items-center justify-between gap-4">
              <div class="min-w-0">
                <p class="font-mono text-sm">{setting.key}</p>
              </div>
              <div class="flex items-center gap-2 shrink-0">
                {#if setting.value === "true" || setting.value === "false"}
                  <button
                    class="px-3 py-1 rounded text-sm font-medium transition
                      {setting.value === 'true'
                        ? 'bg-green-500/20 text-[var(--admin-success)] hover:bg-green-500/30'
                        : 'bg-red-500/20 text-destructive hover:bg-red-500/30'}"
                    onclick={() => updateSetting(setting.key, setting.value === "true" ? "false" : "true")}
                  >
                    {setting.value === "true" ? t('admin.settings.enabled') : t('admin.settings.disabled')}
                  </button>
                {:else}
                  <input
                    type="text"
                    value={setting.value}
                    class="bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded px-3 py-1 text-sm w-48"
                    onchange={(e) => updateSetting(setting.key, (e.target as HTMLInputElement).value)}
                  />
                {/if}
              </div>
            </div>
          {/each}
          {#if settings.length === 0}
            <p class="p-4 text-[hsl(var(--muted-foreground))]">{t('admin.settings.noSettings')}</p>
          {/if}
        </div>
      {/if}

      <!-- Blocked Domains -->
      {#if activeTab === "blocked"}
        <div class="bg-[hsl(var(--card))] rounded-lg p-4 space-y-4">
          <form class="flex gap-2 flex-wrap" onsubmit={(e) => { e.preventDefault(); blockDomain(); }}>
            <input
              type="text"
              bind:value={blockDomainInput}
              placeholder="{t('admin.blocked.domainPlaceholder')}"
              class="bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded px-3 py-2 text-sm flex-1 min-w-[200px]"
            />
            <input
              type="text"
              bind:value={blockReasonInput}
              placeholder="{t('admin.blocked.reasonPlaceholder')}"
              class="bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded px-3 py-2 text-sm flex-1 min-w-[200px]"
            />
            <button
              type="submit"
              class="bg-red-500 hover:bg-red-600 text-white px-4 py-2 rounded text-sm font-medium transition"
            >
              {t('admin.blocked.block')}
            </button>
          </form>

          <!-- Import / Export -->
          <div class="flex gap-2 items-center">
            <button
              class="bg-[hsl(var(--secondary))] hover:bg-[hsl(var(--secondary))]/80 text-[hsl(var(--foreground))] px-3 py-1.5 rounded text-sm transition"
              onclick={exportBlocklist}
            >
              {t('admin.blocked.exportJson')}
            </button>
            <input
              type="file"
              accept=".json"
              class="hidden"
              bind:this={importFileInput}
              onchange={importBlocklist}
            />
            <button
              class="bg-[hsl(var(--secondary))] hover:bg-[hsl(var(--secondary))]/80 text-[hsl(var(--foreground))] px-3 py-1.5 rounded text-sm transition"
              onclick={() => importFileInput?.click()}
            >
              {t('admin.blocked.importJson')}
            </button>
          </div>

          <div class="divide-y divide-[hsl(var(--border))]">
            {#each blockedDomains as domain}
              <div class="py-3 flex items-center justify-between">
                <div>
                  <p class="font-mono text-sm">{domain.domain}</p>
                  {#if domain.reason}
                    <p class="text-xs text-[hsl(var(--muted-foreground))]">{domain.reason}</p>
                  {/if}
                </div>
                <button
                  class="text-xs text-destructive hover:underline"
                  onclick={() => unblockDomain(domain.id)}
                >
                  {t('admin.blocked.unblock')}
                </button>
              </div>
            {/each}
            {#if blockedDomains.length === 0}
              <p class="py-3 text-[hsl(var(--muted-foreground))] text-sm">{t('admin.blocked.noDomains')}</p>
            {/if}
          </div>
        </div>
      {/if}

      <!-- Known Instances -->
      {#if activeTab === "instances"}
        <div class="bg-[hsl(var(--card))] rounded-lg overflow-hidden">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b border-[hsl(var(--border))]">
                <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.instances.domain')}</th>
                <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.instances.tracks')}</th>
                <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.instances.status')}</th>
              </tr>
            </thead>
            <tbody>
              {#each instances as instance}
                <tr class="border-b border-[hsl(var(--border))] last:border-b-0">
                  <td class="p-3 font-mono">{instance.domain}</td>
                  <td class="p-3 text-center">{instance.track_count}</td>
                  <td class="p-3 text-center">
                    {#if instance.is_blocked}
                      <span class="text-destructive text-xs font-medium">{t('admin.instances.blocked')}</span>
                    {:else}
                      <span class="text-[var(--admin-success)] text-xs font-medium">{t('admin.instances.active')}</span>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if instances.length === 0}
            <p class="p-4 text-[hsl(var(--muted-foreground))] text-sm">{t('admin.instances.noInstances')}</p>
          {/if}
        </div>
      {/if}

      <!-- P2P Status -->
      {#if activeTab === "p2p-status"}
        <div class="space-y-6">
          {#if p2pStatus}
            <!-- Status + Node ID -->
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.title')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight {p2pStatus.enabled ? 'text-[var(--admin-success)]' : 'text-[hsl(var(--muted-foreground))]'}">
                  {p2pStatus.enabled ? t('admin.p2p.enabled') : t('admin.p2p.disabled')}
                </p>
              </div>
              {#if p2pStatus.node_id}
                <div class="admin-panel sm:col-span-3">
                  <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.nodeId')}</p>
                  <p class="text-sm font-mono mt-1 truncate select-all">{p2pStatus.node_id}</p>
                </div>
              {/if}
            </div>

            <!-- Relay + Connection Info -->
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <div class="admin-panel sm:col-span-2">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.relayUrl')}</p>
                {#if p2pStatus.relay_connected && p2pStatus.relay_url}
                  <p class="text-sm font-mono mt-1 text-[var(--admin-success)] truncate">{p2pStatus.relay_url}</p>
                {:else}
                  <p class="text-sm mt-1 text-[hsl(var(--muted-foreground))] italic">{t('admin.p2p.relayDisconnected')}</p>
                {/if}
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">Relay</p>
                <p class="text-xl font-bold mt-1 {p2pStatus.relay_connected ? 'text-[var(--admin-success)]' : 'text-destructive'}">
                  {p2pStatus.relay_connected ? t('admin.p2p.relayConnected') : t('admin.p2p.relayDisconnected')}
                </p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.directAddresses')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{p2pStatus.direct_addresses}</p>
              </div>
            </div>

            <!-- Discovery -->
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.dhtDiscovery')}</p>
                <p class="text-xl font-bold mt-1 {p2pStatus.dht_discovery_enabled ? 'text-[var(--admin-success)]' : 'text-[hsl(var(--muted-foreground))]'}">
                  {p2pStatus.dht_discovery_enabled ? t('admin.p2p.dhtEnabled') : t('admin.p2p.dhtDisabled')}
                </p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.rendezvous')}</p>
                <p class="text-xl font-bold mt-1 {p2pStatus.rendezvous_configured ? 'text-[var(--admin-success)]' : 'text-[hsl(var(--muted-foreground))]'}">
                  {p2pStatus.rendezvous_configured ? t('admin.p2p.dhtEnabled') : t('admin.p2p.dhtDisabled')}
                </p>
              </div>
            </div>

            <!-- Peers -->
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.peers')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight">{p2pStatus.peer_count}</p>
              </div>
              <div class="admin-panel">
                <p class="text-xs font-medium text-muted-foreground">{t('admin.p2p.onlinePeers')}</p>
                <p class="mt-1 text-2xl font-semibold tabular-nums tracking-tight text-[var(--admin-success)]">{p2pStatus.online_peer_count}</p>
              </div>
            </div>
          {/if}

          <!-- Add Peer -->
          <div class="admin-panel">
            <h3 class="text-sm font-medium mb-3">{t('admin.p2p.addPeer')}</h3>
            <div class="flex gap-2">
              <input
                type="text"
                bind:value={addPeerInput}
                placeholder={t('admin.p2p.addPeerPlaceholder')}
                class="flex-1 bg-[hsl(var(--input))] border border-[hsl(var(--border))] rounded px-3 py-2 text-sm font-mono"
              />
              <button
                class="bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] px-4 py-2 rounded text-sm font-medium hover:opacity-90 transition"
                disabled={changingPeer || !addPeerInput.trim()} onclick={addPeer}
              >
                {t('admin.p2p.addPeer')}
              </button>
            </div>
            {#if p2pStatus?.explicit_catalog}
              <label class="mt-3 inline-flex items-center gap-2 text-sm">
                <input type="checkbox" bind:checked={followNewPeer} disabled={changingPeer} class="size-4 accent-primary" />
                {t("admin.p2p.followCatalog")}
              </label>
            {/if}
            {#if peerNotice}<p role="status" class="mt-3 text-xs text-muted-foreground">{peerNotice}</p>{/if}
          </div>

          <!-- Peer List -->
          {#if p2pPeers.length > 0}
            <div class="bg-[hsl(var(--card))] rounded-lg overflow-x-auto">
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b border-[hsl(var(--border))]">
                    <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.p2p.nodeId')}</th>
                    <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">Version</th>
                    <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.p2p.peerTracks')}</th>
                    <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">Status</th>
                    <th class="text-right p-3 text-[hsl(var(--muted-foreground))]">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {#each p2pPeers as peer (peer.node_id)}
                    <tr class="border-b border-[hsl(var(--border))] last:border-b-0">
                      <td class="p-3 font-mono text-xs truncate max-w-[300px]">
                        {peer.name ? `${peer.name} (${peer.node_id.slice(0, 12)}…)` : peer.node_id}
                      </td>
                      <td class="p-3 text-center font-mono text-xs text-[hsl(var(--muted-foreground))]">
                        {peer.version ?? '—'}
                      </td>
                      <td class="p-3 text-center">{peer.track_count}</td>
                      <td class="p-3 text-center">
                        {#if peer.is_online}
                          <span class="text-xs text-[var(--admin-success)]">{t('admin.remote.online')}</span>
                        {:else}
                          <span class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.remote.offline')}</span>
                        {/if}
                      </td>
                      <td class="p-3 text-right space-x-2">
                        {#if peer.catalog_followed != null}
                          <button type="button" disabled={changingPeer} class="text-xs text-primary hover:underline disabled:opacity-50"
                            aria-label={t(peer.catalog_followed ? "admin.p2p.unfollowCatalog" : "admin.p2p.followCatalog") + " · " + (peer.name || peer.node_id)}
                            onclick={() => changePeer(() => api.put(`/admin/p2p/peers/${encodeURIComponent(peer.node_id)}/subscription`, { followed: !peer.catalog_followed }))}>
                            {t(peer.catalog_followed ? "admin.p2p.unfollowCatalog" : "admin.p2p.followCatalog")}
                          </button>
                        {/if}
                        <button
                          class="text-xs text-primary hover:underline"
                          onclick={async () => {
                            try {
                              await api.post(`/admin/p2p/peers/${peer.node_id}/ping`);
                              await loadData();
                            } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
                          }}
                        >
                          {t('admin.p2p.ping')}
                        </button>
                        <button
                          class="text-xs text-destructive hover:underline"
                          onclick={async () => {
                            try {
                              await api.delete(`/admin/p2p/peers/${peer.node_id}`);
                              p2pPeers = p2pPeers.filter(p => p.node_id !== peer.node_id);
                            } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
                          }}
                        >
                          {t('admin.p2p.remove')}
                        </button>
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {:else}
            <p class="text-[hsl(var(--muted-foreground))] text-sm">{t('admin.p2p.noPeers')}</p>
          {/if}
        </div>
      {/if}

      <!-- Network Graph -->
      {#if activeTab === "network-graph"}
        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <h2 class="text-lg font-semibold">{t('admin.graph.title')}</h2>
            <button
              class="text-xs bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] px-3 py-1.5 rounded hover:opacity-90 transition"
              onclick={() => loadData()}
            >
              {t('admin.graph.refresh')}
            </button>
          </div>
          {#if networkGraphData.nodes.length === 0}
            <div class="bg-[hsl(var(--card))] rounded-lg p-8 text-center">
              <p class="text-[hsl(var(--muted-foreground))]">{t('admin.graph.empty')}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-2">{t('admin.graph.emptyHint')}</p>
            </div>
          {:else}
            <NetworkGraph bind:data={networkGraphData} />
            <div class="admin-metrics">
              <div class="bg-[hsl(var(--card))] rounded-lg p-3 text-center">
                <p class="text-xs text-[hsl(var(--muted-foreground))] uppercase">{t('admin.graph.totalNodes')}</p>
                <p class="text-2xl font-bold mt-1">{networkGraphData.nodes.length}</p>
              </div>
              <div class="bg-[hsl(var(--card))] rounded-lg p-3 text-center">
                <p class="text-xs text-[hsl(var(--muted-foreground))] uppercase">{t('admin.graph.totalLinks')}</p>
                <p class="text-2xl font-bold mt-1">{networkGraphData.links.length}</p>
              </div>
              <div class="bg-[hsl(var(--card))] rounded-lg p-3 text-center">
                <p class="text-xs text-[hsl(var(--muted-foreground))] uppercase">{t('admin.graph.peerCount')}</p>
                <p class="text-2xl font-bold mt-1 text-[var(--admin-special)]">{networkGraphData.nodes.filter(n => n.node_type === 'peer').length}</p>
              </div>
              <div class="bg-[hsl(var(--card))] rounded-lg p-3 text-center">
                <p class="text-xs text-[hsl(var(--muted-foreground))] uppercase">{t('admin.graph.relayCount')}</p>
                <p class="text-2xl font-bold mt-1 text-primary">{networkGraphData.nodes.filter(n => n.node_type === 'relay').length}</p>
              </div>
            </div>
          {/if}
        </div>
      {/if}

      <!-- P2P Logs -->
      {#if activeTab === "p2p-logs"}
        <div class="space-y-4">
          <!-- Controls -->
          <div class="flex flex-wrap items-center justify-between gap-3">
            <div class="flex items-center gap-3">
              <h2 class="text-lg font-semibold">{t('admin.p2pLogs.title')}</h2>
              <span class="text-xs text-[hsl(var(--muted-foreground))] bg-[hsl(var(--secondary))] px-2 py-0.5 rounded">
                {p2pLogsTotalInBuffer} {t('admin.p2pLogs.entries')}
              </span>
            </div>
            <div class="flex items-center gap-2">
              <!-- Level filter -->
              <select
                class="bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded px-2 py-1.5 text-xs border border-[hsl(var(--border))]"
                bind:value={p2pLogsLevelFilter}
                onchange={() => loadData()}
              >
                <option value="">{t('admin.p2pLogs.allLevels')}</option>
                <option value="ERROR">ERROR</option>
                <option value="WARN">WARN</option>
                <option value="INFO">INFO</option>
                <option value="DEBUG">DEBUG</option>
              </select>
              <!-- Auto-refresh toggle -->
              <button
                class="text-xs px-3 py-1.5 rounded border transition {p2pLogsAutoRefresh
                  ? 'bg-green-500/20 border-green-500/30 text-[var(--admin-success)]'
                  : 'bg-[hsl(var(--secondary))] border-[hsl(var(--border))] text-[hsl(var(--muted-foreground))]'}"
                onclick={() => {
                  p2pLogsAutoRefresh = !p2pLogsAutoRefresh;
                  if (p2pLogsAutoRefresh) {
                    p2pLogsInterval = setInterval(() => loadData(), 5000);
                  } else if (p2pLogsInterval) {
                    clearInterval(p2pLogsInterval);
                    p2pLogsInterval = null;
                  }
                }}
              >
                {p2pLogsAutoRefresh ? t('admin.p2pLogs.autoRefreshOn') : t('admin.p2pLogs.autoRefresh')}
              </button>
              <!-- Manual refresh -->
              <button
                class="text-xs bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] px-3 py-1.5 rounded hover:opacity-90 transition"
                onclick={() => loadData()}
              >
                {t('admin.p2pLogs.refresh')}
              </button>
              <!-- Clear logs -->
              <button
                class="text-xs bg-red-500/20 text-destructive px-3 py-1.5 rounded hover:bg-red-500/30 transition"
                onclick={async () => {
                  try {
                    await api.delete("/admin/p2p/logs");
                    p2pLogs = [];
                    p2pLogsTotalInBuffer = 0;
                  } catch (e: unknown) { error = e instanceof Error ? e.message : String(e); }
                }}
              >
                {t('admin.p2pLogs.clear')}
              </button>
            </div>
          </div>

          <!-- Log entries -->
          {#if p2pLogs.length === 0}
            <div class="bg-[hsl(var(--card))] rounded-lg p-8 text-center">
              <p class="text-[hsl(var(--muted-foreground))]">{t('admin.p2pLogs.empty')}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-2">{t('admin.p2pLogs.emptyHint')}</p>
            </div>
          {:else}
            <div class="bg-muted rounded-lg border border-[hsl(var(--border))] overflow-hidden">
              <div class="max-h-[600px] overflow-y-auto font-mono text-xs">
                {#each p2pLogs as entry, i}
                  <div class="flex items-start gap-2 px-3 py-1.5 hover:bg-secondary transition-colors border-b border-[hsl(var(--border))]/30 {i === p2pLogs.length - 1 ? 'border-b-0' : ''}">
                    <!-- Timestamp -->
                    <span class="text-[hsl(var(--muted-foreground))] whitespace-nowrap flex-shrink-0 select-all">
                      {entry.timestamp.replace('T', ' ').replace('Z', '').slice(11, 23)}
                    </span>
                    <!-- Level badge -->
                    <span class="flex-shrink-0 w-14 text-center rounded px-1 py-0.5 font-bold {
                      entry.level === 'ERROR' ? 'bg-red-500/20 text-destructive' :
                      entry.level === 'WARN' ? 'bg-yellow-500/20 text-[var(--admin-warning)]' :
                      entry.level === 'INFO' ? 'bg-blue-500/20 text-primary' :
                      entry.level === 'DEBUG' ? 'bg-gray-500/20 text-gray-400' :
                      'bg-gray-500/10 text-gray-500'
                    }">
                      {entry.level}
                    </span>
                    <!-- Target (shortened) -->
                    <span class="text-[var(--admin-special)] flex-shrink-0 whitespace-nowrap max-w-[180px] truncate" title={entry.target}>
                      {entry.target.replace('soundtime_p2p::', '').replace('iroh::', 'iroh/')}
                    </span>
                    <!-- Message + fields -->
                    <span class="text-[hsl(var(--foreground))] break-all min-w-0">
                      {entry.message}
                      {#if entry.fields && entry.fields.length > 0}
                        <span class="text-[hsl(var(--muted-foreground))] ml-1">
                          {entry.fields.join(' ')}
                        </span>
                      {/if}
                    </span>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Users -->
      {#if activeTab === "users"}
        <div class="bg-[hsl(var(--card))] rounded-lg overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b border-[hsl(var(--border))]">
                <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.users.user')}</th>
                <th class="text-left p-3 text-[hsl(var(--muted-foreground))]">{t('admin.users.email')}</th>
                <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.users.role')}</th>
                <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.users.status')}</th>
                <th class="text-center p-3 text-[hsl(var(--muted-foreground))]">{t('admin.users.actions')}</th>
                <th class="text-right p-3 text-[hsl(var(--muted-foreground))]">{t('admin.users.registration')}</th>
              </tr>
            </thead>
            <tbody>
              {#each users as u}
                <tr class="border-b border-[hsl(var(--border))] last:border-b-0 {u.is_banned ? 'bg-red-500/5' : ''}">
                  <td class="p-3 font-medium">
                    {u.display_name || u.username}
                    {#if u.is_banned}
                      <span class="ml-1 text-[10px] bg-red-500/20 text-destructive px-1.5 py-0.5 rounded-full font-normal">{t('admin.users.banned').toLowerCase()}</span>
                    {/if}
                  </td>
                  <td class="p-3 text-[hsl(var(--muted-foreground))]">{u.email}</td>
                  <td class="p-3 text-center">
                    <select
                      class="bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded px-2 py-1 text-xs"
                      value={u.role}
                      onchange={(e) => updateUserRole(u.id, (e.target as HTMLSelectElement).value)}
                    >
                      <option value="user">user</option>
                      <option value="admin">admin</option>
                    </select>
                  </td>
                  <td class="p-3 text-center">
                    {#if u.is_banned}
                      <span class="text-xs text-destructive" title={u.ban_reason ?? ""}>{t('admin.users.banned')}</span>
                      {#if u.ban_reason}
                        <p class="text-[10px] text-[hsl(var(--muted-foreground))] mt-0.5 max-w-[150px] truncate" title={u.ban_reason}>{u.ban_reason}</p>
                      {/if}
                    {:else}
                      <span class="text-xs text-[var(--admin-success)]">{t('admin.users.active')}</span>
                    {/if}
                  </td>
                  <td class="p-3 text-center">
                    {#if u.role !== "admin"}
                      {#if u.is_banned}
                        <button
                          class="text-xs px-3 py-1 rounded bg-green-500/20 text-[var(--admin-success)] hover:bg-green-500/30 transition"
                          onclick={() => unbanUser(u.id)}
                        >{t('admin.users.unban')}</button>
                      {:else}
                        <button
                          class="text-xs px-3 py-1 rounded bg-red-500/20 text-destructive hover:bg-red-500/30 transition"
                          onclick={() => banUser(u.id)}
                        >{t('admin.users.ban')}</button>
                      {/if}
                    {:else}
                      <span class="text-xs text-[hsl(var(--muted-foreground))]">—</span>
                    {/if}
                  </td>
                  <td class="p-3 text-right text-xs text-[hsl(var(--muted-foreground))]">
                    {new Date(u.created_at).toLocaleDateString(undefined)}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if users.length === 0}
            <p class="p-4 text-[hsl(var(--muted-foreground))] text-sm">{t('admin.users.noUsers')}</p>
          {/if}
        </div>
      {/if}

      <!-- IA Éditoriale -->
      {#if activeTab === "editorial"}
      <div class="space-y-6">
        <!-- AI Configuration -->
        <div class="admin-panel">
          <h2 class="text-lg font-semibold mb-4">{t('admin.editorial.apiConfig')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))] mb-4">
            {t('admin.editorial.apiConfigDesc')}
          </p>

          <div class="space-y-4 max-w-lg">
            <div>
              <label class="block text-sm font-medium mb-1" for="ai-key">{t('admin.editorial.apiKey')}</label>
              <input
                id="ai-key"
                type="password"
                bind:value={editorialApiKey}
                placeholder="sk-..."
                class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              />
            </div>
            <div>
              <label class="block text-sm font-medium mb-1" for="ai-url">{t('admin.editorial.baseUrl')}</label>
              <input
                id="ai-url"
                type="text"
                bind:value={editorialBaseUrl}
                placeholder="https://api.openai.com/v1"
                class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              />
            </div>
            <div>
              <label class="block text-sm font-medium mb-1" for="ai-model">{t('admin.editorial.model')}</label>
              <input
                id="ai-model"
                type="text"
                bind:value={editorialModel}
                placeholder="gpt-4o-mini"
                class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              />
            </div>
            <div>
              <label class="block text-sm font-medium mb-1" for="ai-max-tracks">{t('admin.editorial.maxTracks')}</label>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mb-1">{t('admin.editorial.maxTracksDesc')}</p>
              <input
                id="ai-max-tracks"
                type="number"
                bind:value={editorialMaxTracks}
                placeholder="500"
                min="10"
                max="5000"
                class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              />
            </div>
            <div>
              <label class="block text-sm font-medium mb-1" for="ai-language">{t('admin.editorial.instanceLanguage')}</label>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mb-1">{t('admin.editorial.instanceLanguageDesc')}</p>
              <input
                id="ai-language"
                type="text"
                bind:value={editorialInstanceLanguage}
                placeholder="English"
                class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              />
            </div>
            <button
              class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition"
              onclick={() => saveEditorialSettings()}
            >
              {t('admin.editorial.saveConfig')}
            </button>
          </div>
        </div>

        <!-- Generation Panel -->
        {#if editorialStatus?.ai_configured}
          <div class="admin-panel">
            <h2 class="text-lg font-semibold mb-4">{t('admin.editorial.playlistGeneration')}</h2>

            <div class="admin-metrics mb-6">
              <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
                <p class="text-2xl font-bold text-[hsl(var(--primary))]">{editorialStatus.playlist_count}</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.editorial.editorialPlaylists')}</p>
              </div>
              <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
                <p class="text-sm font-medium">
                  {editorialStatus.last_generated
                    ? new Date(editorialStatus.last_generated).toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit" })
                    : t('admin.editorial.never')}
                </p>
                <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.editorial.lastGenerated')}</p>
              </div>
              <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
                <p class="text-sm font-medium">
                  {editorialStatus.ai_model}
                </p>
                <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.editorial.modelUsed')}</p>
              </div>
            </div>

            {#if editorialStatus.needs_regeneration}
              <div class="bg-yellow-500/10 border border-yellow-500/30 rounded-lg p-3 mb-4">
                <p class="text-sm text-[var(--admin-warning)]">{t('admin.editorial.regenerationNeeded')}</p>
              </div>
            {/if}

            <div class="flex gap-3">
              <button
                class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition disabled:opacity-50 flex items-center gap-2"
                disabled={editorialGenerating}
                onclick={() => generateEditorialPlaylists()}
              >
                {#if editorialGenerating}
                  <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
                  {t('admin.editorial.generating')}
                {:else}
                  {t('admin.editorial.generatePlaylists')}
                {/if}
              </button>
            </div>

            <p class="text-xs text-[hsl(var(--muted-foreground))] mt-3">
              {t('admin.editorial.generationHint')}
            </p>
          </div>
        {/if}
      </div>
      {/if}

      <!-- Paroles (Lyrics) -->
      {#if activeTab === "lyrics"}
      <div class="space-y-6">
        <div class="admin-panel">
          <h2 class="text-lg font-semibold mb-4">{t('admin.lyrics.configTitle')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))] mb-4">
            {t('admin.lyrics.configDesc')}
          </p>

          <div class="space-y-4 max-w-lg">
            <div>
              <label class="block text-sm font-medium mb-1" for="lyrics-provider">{t('admin.lyrics.provider')}</label>
              <select
                id="lyrics-provider"
                bind:value={lyricsProvider}
                class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              >
                <option value="none">{t('admin.lyrics.disabled')}</option>
                <option value="musixmatch">Musixmatch</option>
                <option value="lyricscom">Lyrics.com</option>
              </select>
            </div>

            {#if lyricsProvider === "musixmatch"}
              <div>
                <label class="block text-sm font-medium mb-1" for="lyrics-mm-key">{t('admin.lyrics.musixmatchKey')}</label>
                <input
                  id="lyrics-mm-key"
                  type="password"
                  bind:value={lyricsMusixmatchKey}
                  placeholder="{t('admin.lyrics.musixmatchPlaceholder')}"
                  class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
                />
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">
                  {t('admin.lyrics.musixmatchHint')} <a href="https://developer.musixmatch.com/" target="_blank" class="text-[hsl(var(--primary))] hover:underline">developer.musixmatch.com</a>
                </p>
              </div>
            {/if}

            {#if lyricsProvider === "lyricscom"}
              <div>
                <label class="block text-sm font-medium mb-1" for="lyrics-lc-key">{t('admin.lyrics.lyricscomKey')}</label>
                <input
                  id="lyrics-lc-key"
                  type="password"
                  bind:value={lyricsLyricscomKey}
                  placeholder="{t('admin.lyrics.lyricscomPlaceholder')}"
                  class="w-full bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
                />
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">
                  {t('admin.lyrics.lyricscomHint')} <a href="https://www.lyrics.com/lyrics_api.php" target="_blank" class="text-[hsl(var(--primary))] hover:underline">lyrics.com/lyrics_api.php</a>
                </p>
              </div>
            {/if}

            <button
              class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition disabled:opacity-50 flex items-center gap-2"
              disabled={lyricsSaving}
              onclick={() => saveLyricsSettings()}
            >
              {#if lyricsSaving}
                <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
                {t('admin.lyrics.saving')}
              {:else}
                {t('admin.lyrics.saveConfig')}
              {/if}
            </button>
          </div>
        </div>
      </div>
      {/if}

      <!-- Signalements -->
      {#if activeTab === "reports"}
      <div class="space-y-6">
        <!-- Stats Cards -->
        {#if reportStats}
          <div class="admin-metrics">
            <div class="admin-panel text-center">
              <p class="text-2xl font-bold text-[var(--admin-warning)]">{reportStats.pending}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.reports.pending')}</p>
            </div>
            <div class="admin-panel text-center">
              <p class="text-2xl font-bold text-[var(--admin-success)]">{reportStats.resolved}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.reports.resolved')}</p>
            </div>
            <div class="admin-panel text-center">
              <p class="text-2xl font-bold text-[hsl(var(--muted-foreground))]">{reportStats.dismissed}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.reports.dismissed')}</p>
            </div>
            <div class="admin-panel text-center">
              <p class="text-2xl font-bold">{reportStats.total}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.reports.total')}</p>
            </div>
          </div>
        {/if}

        <!-- Filter -->
        <div class="flex gap-2">
          {#each [{ id: "all", label: t('admin.reports.filterAll') }, { id: "pending", label: t('admin.reports.filterPending') }, { id: "resolved", label: t('admin.reports.filterResolved') }, { id: "dismissed", label: t('admin.reports.filterDismissed') }] as f}
            <button
              class="px-3 py-1.5 text-xs rounded-lg transition {reportFilter === f.id ? 'bg-[hsl(var(--primary))] text-primary-foreground' : 'bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))] hover:text-foreground'}"
              onclick={() => reportFilter = f.id}
            >{f.label}</button>
          {/each}
        </div>

        <!-- Reports Table -->
        <div class="bg-[hsl(var(--card))] rounded-lg border border-[hsl(var(--border))] overflow-hidden">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b border-[hsl(var(--border))] text-left text-xs text-[hsl(var(--muted-foreground))] uppercase">
                <th class="p-3">{t('admin.reports.track')}</th>
                <th class="p-3">{t('admin.reports.artist')}</th>
                <th class="p-3">{t('admin.reports.type')}</th>
                <th class="p-3">{t('admin.reports.reportedBy')}</th>
                <th class="p-3">{t('admin.reports.reason')}</th>
                <th class="p-3">{t('admin.reports.status')}</th>
                <th class="p-3">{t('admin.reports.date')}</th>
                <th class="p-3 text-right">{t('admin.reports.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {#each filteredReports() as report}
                <tr class="border-b border-[hsl(var(--border))] hover:bg-[hsl(var(--secondary))]/50">
                  <td class="p-3 font-medium max-w-[180px] truncate">{report.track_title}</td>
                  <td class="p-3 text-[hsl(var(--muted-foreground))] max-w-[120px] truncate">{report.track_artist}</td>
                  <td class="p-3">
                    <span class="text-xs px-2 py-0.5 rounded {report.is_local ? 'bg-green-500/20 text-[var(--admin-success)]' : 'bg-blue-500/20 text-primary'}">
                      {report.is_local ? t('admin.reports.local') : t('admin.reports.remote')}
                    </span>
                  </td>
                  <td class="p-3 text-[hsl(var(--muted-foreground))]">{report.reporter_username}</td>
                  <td class="p-3 text-xs text-[hsl(var(--muted-foreground))] max-w-[200px] truncate" title={report.reason}>{report.reason}</td>
                  <td class="p-3">
                    {#if report.status === "pending"}
                      <span class="text-xs px-2 py-0.5 rounded bg-yellow-500/20 text-[var(--admin-warning)]">{t('admin.reports.statusPending')}</span>
                    {:else if report.status === "resolved"}
                      <span class="text-xs px-2 py-0.5 rounded bg-green-500/20 text-[var(--admin-success)]">{t('admin.reports.statusResolved')}</span>
                    {:else}
                      <span class="text-xs px-2 py-0.5 rounded bg-gray-500/20 text-gray-400">{t('admin.reports.statusDismissed')}</span>
                    {/if}
                  </td>
                  <td class="p-3 text-xs text-[hsl(var(--muted-foreground))]">
                    {new Date(report.created_at).toLocaleDateString(undefined)}
                  </td>
                  <td class="p-3 text-right">
                    {#if report.status === "pending"}
                      <div class="flex gap-1 justify-end">
                        <button
                          class="px-2 py-1 text-xs rounded bg-red-500/20 text-destructive hover:bg-red-500/30 transition"
                          title={report.is_local ? t('admin.reports.delete') : t('admin.reports.unlist')}
                          onclick={() => resolveReport(report.id, "resolved", report.is_local ? "delete" : "unlist")}
                        >
                          {report.is_local ? t('admin.reports.delete') : t('admin.reports.unlist')}
                        </button>
                        <button
                          class="px-2 py-1 text-xs rounded bg-green-500/20 text-[var(--admin-success)] hover:bg-green-500/30 transition"
                          onclick={() => resolveReport(report.id, "resolved", "none")}
                        >{t('admin.reports.resolve')}</button>
                        <button
                          class="px-2 py-1 text-xs rounded bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))] hover:opacity-80 transition"
                          onclick={() => resolveReport(report.id, "dismissed")}
                        >{t('admin.reports.dismiss')}</button>
                      </div>
                    {:else if report.admin_note}
                      <span class="text-xs text-[hsl(var(--muted-foreground))]" title={report.admin_note}>📝</span>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if filteredReports().length === 0}
            <p class="p-8 text-center text-[hsl(var(--muted-foreground))] text-sm">{t('admin.reports.noReports')}</p>
          {/if}
        </div>

        <!-- Browse Tracks Section -->
        <div class="admin-panel">
          <h2 class="text-lg font-semibold mb-4">{t('admin.reports.browseTitle')}</h2>
          <div class="flex gap-3 mb-4">
            <input
              type="text"
              bind:value={adminTrackSearch}
              placeholder="{t('admin.reports.searchPlaceholder')}"
              class="flex-1 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              onkeydown={(e) => { if (e.key === "Enter") { adminTrackPage = 1; loadBrowseTracks(); } }}
            />
            <button
              class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition"
              onclick={() => { adminTrackPage = 1; loadBrowseTracks(); }}
            >{t('admin.reports.searchBtn')}</button>
          </div>

          {#if adminTracks.length > 0}
            <table class="w-full text-sm mb-4">
              <thead>
                <tr class="border-b border-[hsl(var(--border))] text-left text-xs text-[hsl(var(--muted-foreground))] uppercase">
                  <th class="p-2">{t('admin.reports.track')}</th>
                  <th class="p-2">{t('admin.reports.artist')}</th>
                  <th class="p-2">{t('admin.reports.type')}</th>
                  <th class="p-2 text-right">{t('admin.reports.trackPlays')}</th>
                  <th class="p-2 text-right">{t('admin.reports.trackReports')}</th>
                  <th class="p-2 text-right">{t('admin.reports.trackAction')}</th>
                </tr>
              </thead>
              <tbody>
                {#each adminTracks as tr}
                  <tr class="border-b border-[hsl(var(--border))] hover:bg-[hsl(var(--secondary))]/50">
                    <td class="p-2 font-medium max-w-[200px] truncate">{tr.title}</td>
                    <td class="p-2 text-[hsl(var(--muted-foreground))]">{tr.artist_name}</td>
                    <td class="p-2">
                      <span class="text-xs px-2 py-0.5 rounded {tr.is_local ? 'bg-green-500/20 text-[var(--admin-success)]' : 'bg-blue-500/20 text-primary'}">
                        {tr.is_local ? t('admin.reports.local') : t('admin.reports.remote')}
                      </span>
                    </td>
                    <td class="p-2 text-right tabular-nums">{tr.play_count}</td>
                    <td class="p-2 text-right">
                      {#if tr.report_count > 0}
                        <span class="text-xs px-2 py-0.5 rounded bg-red-500/20 text-destructive">{tr.report_count}</span>
                      {:else}
                        <span class="text-xs text-[hsl(var(--muted-foreground))]">0</span>
                      {/if}
                    </td>
                    <td class="p-2 text-right">
                      <button
                        class="px-2 py-1 text-xs rounded bg-red-500/20 text-destructive hover:bg-red-500/30 transition disabled:opacity-50"
                        disabled={moderating}
                        onclick={() => moderateTrack(tr.id, tr.title)}
                      >{tr.is_local ? t('admin.reports.delete') : t('admin.reports.unlist')}</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>

            <!-- Pagination -->
            {#if adminTrackTotalPages > 1}
              <div class="flex items-center justify-between text-sm">
                <span class="text-[hsl(var(--muted-foreground))]">{t('admin.reports.pagination', { total: adminTrackTotal, page: adminTrackPage, totalPages: adminTrackTotalPages })}</span>
                <div class="flex gap-2">
                  <button
                    class="px-3 py-1 rounded bg-[hsl(var(--secondary))] text-sm disabled:opacity-50 transition"
                    disabled={adminTrackPage <= 1}
                    onclick={() => { adminTrackPage--; loadBrowseTracks(); }}
                  >{t('admin.reports.previous')}</button>
                  <button
                    class="px-3 py-1 rounded bg-[hsl(var(--secondary))] text-sm disabled:opacity-50 transition"
                    disabled={adminTrackPage >= adminTrackTotalPages}
                    onclick={() => { adminTrackPage++; loadBrowseTracks(); }}
                  >{t('admin.reports.next')}</button>
                </div>
              </div>
            {/if}
          {:else}
            <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('admin.reports.browseHint')}</p>
          {/if}
        </div>
      </div>
      {/if}

      <!-- CGU / Terms of Service -->
      {#if activeTab === "tos"}
      <div class="space-y-6">
        <div class="admin-panel">
          <div class="flex items-center justify-between mb-4">
            <div>
              <h2 class="text-lg font-semibold">{t('admin.tos.title')}</h2>
              <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">
                {#if tosIsDefault}
                  {t('admin.tos.defaultActive')}
                {:else}
                  {t('admin.tos.customVersion')}
                {/if}
              </p>
            </div>
            <a href="/tos" target="_blank" class="text-xs text-[hsl(var(--primary))] hover:underline">{t('admin.tos.viewPublic')}</a>
          </div>

          {#if tosSuccess}
            <div class="bg-green-500/10 border border-green-500/30 rounded-lg p-3 mb-4 text-[var(--admin-success)] text-sm">{tosSuccess}</div>
          {/if}

          <textarea
            bind:value={tosContent}
            class="w-full min-h-[400px] bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg px-4 py-3 text-sm font-mono border border-[hsl(var(--border))] focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))] resize-y"
            placeholder="{t('admin.tos.placeholder')}"
          ></textarea>

          <div class="flex gap-3 mt-4">
            <button
              class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition disabled:opacity-50"
              disabled={tosSaving}
              onclick={() => saveTos()}
            >
              {tosSaving ? t('admin.tos.saving') : t('admin.tos.save')}
            </button>
            <button
              class="px-4 py-2 bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))] rounded-lg text-sm font-medium hover:opacity-80 transition disabled:opacity-50"
              disabled={tosSaving}
              onclick={() => resetTos()}
            >
              {t('admin.tos.restoreDefault')}
            </button>
          </div>
        </div>
      </div>
      {/if}

      {#if activeTab === "storage" || activeTab === "sync" || activeTab === "integrity"}
        <StorageSection tab={activeTab} section={storage} onSyncTab={() => switchTab("sync")} />
      {/if}

      <!-- Plugins -->
      {#if activeTab === "plugins"}
      <div class="space-y-6">
        {#if pluginSuccess}
          <div class="bg-green-500/10 border border-green-500/30 rounded-lg p-4 text-[var(--admin-success)]">
            {pluginSuccess}
            <button class="ml-2 underline" onclick={() => (pluginSuccess = "")}>{t('common.close')}</button>
          </div>
        {/if}

        <!-- Install form -->
        <div class="admin-panel">
          <h2 class="text-lg font-semibold mb-1">{t('admin.plugins.install')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))] mb-4">{t('admin.plugins.installDescription')}</p>
          <div class="flex gap-3">
            <input
              type="url"
              bind:value={pluginInstallUrl}
              placeholder={t('admin.plugins.gitUrlPlaceholder')}
              class="flex-1 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-lg px-4 py-2 text-sm"
            />
            <button
              class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition disabled:opacity-50"
              disabled={pluginInstalling || !pluginInstallUrl.trim()}
              onclick={installPlugin}
            >
              {pluginInstalling ? t('admin.plugins.installing') : t('admin.plugins.install')}
            </button>
          </div>
        </div>

        <!-- Plugin list -->
        {#if plugins.length === 0}
          <div class="text-center py-12 text-[hsl(var(--muted-foreground))]">
            <Puzzle size={48} class="mx-auto mb-4 opacity-30" />
            <p class="font-medium">{t('admin.plugins.noPlugins')}</p>
            <p class="text-sm mt-1">{t('admin.plugins.noPluginsHint')}</p>
          </div>
        {:else}
          <div class="space-y-3">
            {#each plugins as plugin (plugin.id)}
              <div class="admin-panel">
                <div class="flex items-start justify-between">
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-3">
                      <h3 class="font-semibold text-base">{plugin.name}</h3>
                      <span class="text-xs font-mono text-[hsl(var(--muted-foreground))]">v{plugin.version}</span>
                      <span class="px-2 py-0.5 rounded-full text-xs font-medium {
                        plugin.status === 'enabled' ? 'bg-green-500/15 text-[var(--admin-success)]' :
                        plugin.status === 'error' ? 'bg-red-500/15 text-destructive' :
                        'bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))]'
                      }">
                        {t(`admin.plugins.status.${plugin.status}`)}
                      </span>
                    </div>
                    {#if plugin.description}
                      <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">{plugin.description}</p>
                    {/if}
                    <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs text-[hsl(var(--muted-foreground))] mt-2">
                      {#if plugin.author}<span>{t('admin.plugins.author')}: {plugin.author}</span>{/if}
                      {#if plugin.license}<span>{t('admin.plugins.license')}: {plugin.license}</span>{/if}
                      {#if plugin.homepage}<a href={plugin.homepage} target="_blank" rel="noopener" class="underline hover:text-[hsl(var(--foreground))]">{t('admin.plugins.homepage')}</a>{/if}
                      <span>{t('admin.plugins.installedAt')}: {new Date(plugin.installed_at).toLocaleDateString()}</span>
                    </div>
                    {#if plugin.status === "error" && plugin.error_message}
                      <p class="text-xs text-destructive mt-2 font-mono">{plugin.error_message}</p>
                    {/if}
                  </div>
                  <div class="flex items-center gap-2 ml-4 shrink-0">
                    {#if plugin.status === "disabled" || plugin.status === "error"}
                      <button
                        class="px-3 py-1.5 bg-green-600 text-white rounded-lg text-xs font-medium hover:opacity-90 transition"
                        onclick={() => enablePlugin(plugin.id)}
                      >{t('admin.plugins.enable')}</button>
                    {:else}
                      <button
                        class="px-3 py-1.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg text-xs font-medium hover:opacity-90 transition"
                        onclick={() => disablePlugin(plugin.id)}
                      >{t('admin.plugins.disable')}</button>
                    {/if}
                    <button
                      class="px-3 py-1.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg text-xs font-medium hover:opacity-90 transition disabled:opacity-50"
                      disabled={pluginUpdating === plugin.id}
                      onclick={() => updatePlugin(plugin.id)}
                    >{pluginUpdating === plugin.id ? t('admin.plugins.updating') : t('admin.plugins.update')}</button>
                    <button
                      class="px-3 py-1.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-lg text-xs font-medium hover:opacity-90 transition"
                      onclick={() => loadPluginConfig(plugin)}
                    >{t('admin.plugins.configure')}</button>
                    <button
                      class="px-3 py-1.5 bg-red-600 text-white rounded-lg text-xs font-medium hover:opacity-90 transition"
                      onclick={() => uninstallPlugin(plugin.id)}
                    >{t('admin.plugins.uninstall')}</button>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {/if}

        <!-- Plugin detail panel (config + logs) -->
        {#if selectedPlugin}
          <div class="admin-panel space-y-6">
            <div class="flex items-center justify-between">
              <h2 class="text-lg font-semibold">{selectedPlugin.name} — {t('admin.plugins.configure')}</h2>
              <button class="text-sm text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))] underline" onclick={() => (selectedPlugin = null)}>{t('common.close')}</button>
            </div>

            <!-- Permissions -->
            <div>
              <h3 class="text-sm font-medium mb-2">{t('admin.plugins.permissions')}</h3>
              <div class="grid grid-cols-2 sm:grid-cols-3 gap-2 text-xs">
                {#if selectedPlugin.permissions.http_hosts.length > 0}
                  <div class="bg-[hsl(var(--secondary))] rounded px-3 py-2">
                    <span class="text-[hsl(var(--muted-foreground))]">{t('admin.plugins.httpHosts')}:</span>
                    <span class="ml-1 font-mono">{selectedPlugin.permissions.http_hosts.join(", ")}</span>
                  </div>
                {/if}
                {#if selectedPlugin.permissions.events.length > 0}
                  <div class="bg-[hsl(var(--secondary))] rounded px-3 py-2">
                    <span class="text-[hsl(var(--muted-foreground))]">{t('admin.plugins.events')}:</span>
                    <span class="ml-1 font-mono">{selectedPlugin.permissions.events.join(", ")}</span>
                  </div>
                {/if}
                {#if selectedPlugin.permissions.write_tracks}
                  <div class="bg-yellow-500/10 border border-yellow-500/30 rounded px-3 py-2">{t('admin.plugins.writeTracks')}: ✓</div>
                {/if}
                {#if selectedPlugin.permissions.config_access}
                  <div class="bg-[hsl(var(--secondary))] rounded px-3 py-2">{t('admin.plugins.configAccess')}: ✓</div>
                {/if}
                {#if selectedPlugin.permissions.read_users}
                  <div class="bg-yellow-500/10 border border-yellow-500/30 rounded px-3 py-2">{t('admin.plugins.readUsers')}: ✓</div>
                {/if}
              </div>
            </div>

            <!-- Configuration editor -->
            <div>
              <h3 class="text-sm font-medium mb-2">{t('admin.plugins.config')}</h3>
              {#if pluginConfig.length === 0}
                <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('admin.plugins.noConfig')}</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{t('admin.plugins.noConfigHint')}</p>
              {:else}
                <div class="space-y-2">
                  {#each pluginConfig as entry, i}
                    <div class="flex gap-2 items-center">
                      <input
                        type="text"
                        value={entry.key}
                        readonly
                        class="w-1/3 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded px-3 py-1.5 text-xs font-mono"
                      />
                      <input
                        type="text"
                        bind:value={pluginConfig[i].value}
                        class="flex-1 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded px-3 py-1.5 text-xs"
                      />
                      <button
                        class="text-destructive hover:text-red-300 text-xs"
                        onclick={() => removePluginConfigEntry(entry.key)}
                      >{t('common.delete')}</button>
                    </div>
                  {/each}
                </div>
              {/if}
              <div class="flex gap-2 items-center mt-3">
                <input
                  type="text"
                  bind:value={pluginConfigNewKey}
                  placeholder={t('admin.plugins.addConfigKey')}
                  class="w-1/3 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded px-3 py-1.5 text-xs"
                />
                <input
                  type="text"
                  bind:value={pluginConfigNewValue}
                  placeholder={t('admin.plugins.addConfigValue')}
                  class="flex-1 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded px-3 py-1.5 text-xs"
                />
                <button
                  class="px-3 py-1.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded text-xs font-medium hover:opacity-90 transition"
                  onclick={addPluginConfigEntry}
                >{t('admin.plugins.addConfig')}</button>
              </div>
              <button
                class="mt-3 px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition"
                onclick={savePluginConfig}
              >{t('admin.plugins.saveConfig')}</button>
            </div>

            <!-- Event logs -->
            <div>
              <h3 class="text-sm font-medium mb-2">{t('admin.plugins.logs')} ({pluginLogsTotal})</h3>
              {#if pluginLogs.length === 0}
                <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('admin.plugins.noLogs')}</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{t('admin.plugins.noLogsHint')}</p>
              {:else}
                <div class="overflow-x-auto">
                  <table class="w-full text-xs">
                    <thead>
                      <tr class="border-b border-[hsl(var(--border))]">
                        <th class="text-left py-2 px-2 font-medium text-[hsl(var(--muted-foreground))]">{t('admin.plugins.eventName')}</th>
                        <th class="text-left py-2 px-2 font-medium text-[hsl(var(--muted-foreground))]">{t('admin.plugins.result')}</th>
                        <th class="text-right py-2 px-2 font-medium text-[hsl(var(--muted-foreground))]">{t('admin.plugins.executionTime')}</th>
                        <th class="text-right py-2 px-2 font-medium text-[hsl(var(--muted-foreground))]">Date</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each pluginLogs as log (log.id)}
                        <tr class="border-b border-[hsl(var(--border))]/50">
                          <td class="py-2 px-2 font-mono">{log.event_name}</td>
                          <td class="py-2 px-2">
                            <span class="px-1.5 py-0.5 rounded text-[10px] font-medium {
                              log.result === 'success' ? 'bg-green-500/15 text-[var(--admin-success)]' :
                              log.result === 'error' ? 'bg-red-500/15 text-destructive' :
                              'bg-yellow-500/15 text-[var(--admin-warning)]'
                            }">{log.result}</span>
                          </td>
                          <td class="py-2 px-2 text-right font-mono">{log.execution_time_ms}ms</td>
                          <td class="py-2 px-2 text-right text-[hsl(var(--muted-foreground))]">{new Date(log.created_at).toLocaleString()}</td>
                        </tr>
                        {#if log.error_message}
                          <tr><td colspan="4" class="px-2 pb-2 text-destructive font-mono text-[10px]">{log.error_message}</td></tr>
                        {/if}
                      {/each}
                    </tbody>
                  </table>
                </div>
                {#if pluginLogsTotal > 20}
                  <div class="flex justify-center gap-2 mt-3">
                    <button
                      class="px-3 py-1 bg-[hsl(var(--secondary))] rounded text-xs disabled:opacity-50"
                      disabled={pluginLogsPage <= 1}
                      onclick={() => loadPluginLogs(pluginLogsPage - 1)}
                    >← {t('admin.reports.previous')}</button>
                    <span class="text-xs text-[hsl(var(--muted-foreground))] py-1">{pluginLogsPage} / {Math.ceil(pluginLogsTotal / 20)}</span>
                    <button
                      class="px-3 py-1 bg-[hsl(var(--secondary))] rounded text-xs disabled:opacity-50"
                      disabled={pluginLogsPage >= Math.ceil(pluginLogsTotal / 20)}
                      onclick={() => loadPluginLogs(pluginLogsPage + 1)}
                    >{t('admin.reports.next')} →</button>
                  </div>
                {/if}
              {/if}
            </div>
          </div>
        {/if}
      </div>
      {/if}

      {#if activeTab === "themes"}
      <div class="space-y-6">
        <h2 class="text-xl font-semibold">{t("admin.themes.title")}</h2>

        <!-- Success banner -->
        {#if themeSuccess}
          <div class="bg-green-500/10 border border-green-500/30 text-[var(--admin-success)] px-4 py-3 rounded-lg text-sm">
            {themeSuccess}
          </div>
        {/if}

        <!-- Install form -->
        <div class="bg-[hsl(var(--card))] rounded-xl p-5 border border-[hsl(var(--border))]">
          <h3 class="text-sm font-medium mb-1">{t("admin.themes.install")}</h3>
          <p class="text-xs text-[hsl(var(--muted-foreground))] mb-3">{t("admin.themes.installDescription")}</p>
          <div class="flex gap-2">
            <input
              type="text"
              bind:value={themeInstallUrl}
              placeholder={t("admin.themes.gitUrlPlaceholder")}
              class="flex-1 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-[hsl(var(--primary))]"
            />
            <button
              onclick={installTheme}
              disabled={themeInstalling || !themeInstallUrl.trim()}
              class="px-4 py-2 bg-[hsl(var(--primary))] text-primary-foreground rounded-lg text-sm font-medium hover:opacity-90 transition disabled:opacity-50"
            >
              {themeInstalling ? t("admin.themes.installing") : t("admin.themes.install")}
            </button>
          </div>
        </div>

        <!-- Theme list -->
        {#if themes.length === 0}
          <div class="text-center py-12 text-[hsl(var(--muted-foreground))]">
            <Palette class="w-12 h-12 mx-auto mb-3 opacity-40" />
            <p class="font-medium">{t("admin.themes.noThemes")}</p>
            <p class="text-sm mt-1">{t("admin.themes.noThemesHint")}</p>
          </div>
        {:else}
          <div class="space-y-3">
            {#each themes as theme (theme.id)}
              <div class="bg-[hsl(var(--card))] rounded-xl p-5 border border-[hsl(var(--border))]">
                <div class="flex items-start justify-between gap-4">
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2 flex-wrap">
                      <h3 class="font-medium">{theme.name}</h3>
                      <span class="text-xs text-[hsl(var(--muted-foreground))]">v{theme.version}</span>
                      {#if theme.status === "enabled"}
                        <span class="text-[10px] px-2 py-0.5 rounded-full bg-green-500/20 text-[var(--admin-success)] font-medium">
                          {t("admin.themes.activeIndicator")}
                        </span>
                      {:else}
                        <span class="text-[10px] px-2 py-0.5 rounded-full bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))] font-medium">
                          {t("admin.themes.status.disabled")}
                        </span>
                      {/if}
                    </div>
                    {#if theme.description}
                      <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">{theme.description}</p>
                    {/if}
                    <div class="flex flex-wrap gap-x-4 gap-y-1 mt-2 text-xs text-[hsl(var(--muted-foreground))]">
                      {#if theme.author}
                        <span>{t("admin.themes.author")}: {theme.author}</span>
                      {/if}
                      {#if theme.license}
                        <span>{t("admin.themes.license")}: {theme.license}</span>
                      {/if}
                      {#if theme.homepage}
                        <a href={theme.homepage} target="_blank" rel="noopener noreferrer" class="text-[hsl(var(--primary))] hover:underline">
                          {t("admin.themes.homepage")}
                        </a>
                      {/if}
                      <span>{t("admin.themes.installedAt")}: {new Date(theme.installed_at).toLocaleDateString()}</span>
                    </div>
                  </div>
                  <div class="flex items-center gap-2 flex-shrink-0">
                    {#if theme.status === "disabled"}
                      <button
                        onclick={() => enableTheme(theme.id)}
                        class="px-3 py-1.5 bg-green-600 text-white rounded-lg text-xs font-medium hover:opacity-90 transition"
                      >
                        {t("admin.themes.enable")}
                      </button>
                    {:else}
                      <button
                        onclick={() => disableTheme(theme.id)}
                        class="px-3 py-1.5 bg-[hsl(var(--secondary))] rounded-lg text-xs font-medium hover:opacity-80 transition"
                      >
                        {t("admin.themes.disable")}
                      </button>
                    {/if}
                    <button
                      onclick={() => updateTheme(theme.id)}
                      disabled={themeUpdating === theme.id}
                      class="px-3 py-1.5 bg-[hsl(var(--secondary))] rounded-lg text-xs font-medium hover:opacity-80 transition disabled:opacity-50"
                    >
                      {themeUpdating === theme.id ? t("admin.themes.updating") : t("admin.themes.update")}
                    </button>
                    <button
                      onclick={() => uninstallTheme(theme.id)}
                      class="px-3 py-1.5 bg-red-600/20 text-destructive rounded-lg text-xs font-medium hover:bg-red-600/30 transition"
                    >
                      {t("admin.themes.uninstall")}
                    </button>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
      {/if}

        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .admin { container: administration / inline-size; --admin-success: light-dark(#206740,#8ad5aa); --admin-warning: light-dark(#86500e,#ebbc75); --admin-special: light-dark(#68409c,#cda9f3); }
  .admin-layout { display: grid; grid-template-columns: minmax(0,1fr); gap: 1.25rem; min-width: 0; }
  .admin-navigation { display: none; }
  .admin-content { min-width: 0; container: admin-content / inline-size; }
  .admin-metrics { display: grid; grid-template-columns: repeat(auto-fit,minmax(min(100%,9.25rem),1fr)); gap: .7rem; }
  .admin-panel { min-width: 0; padding: 1.05rem; border: 1px solid var(--glass-edge); border-radius: .9rem; background: var(--glass-panel); }
  .admin-metrics .admin-panel { padding: .95rem; }
  .admin-navigation p { font-size: .74rem; font-weight: 550; }
  .admin-content :global(h2) { font-size: 1.16rem; line-height: 1.32; font-weight: 625; letter-spacing: -.02em; }
  @container administration (min-width: 55rem) {
    .admin-layout { grid-template-columns: 10.2rem minmax(0,1fr); gap: 1.6rem; }
    .admin-select { display: none; }
    .admin-navigation { display: block; padding-right: .7rem; border-right: 1px solid hsl(var(--border)); }
  }
  @container admin-content (max-width: 35rem) {
    .admin-content :global(table) { font-size: .78rem; }
  }
</style>
