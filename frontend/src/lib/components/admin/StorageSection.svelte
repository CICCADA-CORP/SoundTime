<script lang="ts">
  import { api } from "$lib/api";
  import { AlertDialog } from "bits-ui";
  import { Button, buttonVariants } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  import type { StorageTab, createStorageSection } from "./storage.svelte";

  let { tab, section, onSyncTab }: {
    tab: StorageTab;
    section: ReturnType<typeof createStorageSection>;
    onSyncTab: () => void;
  } = $props();
  const storageStatus = $derived(section.overview);
  const syncReport = $derived(section.syncReport);
  const integrityReport = $derived(section.integrityReport);
  const storageChecking = $derived(section.running === "integrity");
  const storageSyncing = $derived(section.running === "sync");
  let confirming = $state(false);
  let cleanupTrigger = $state<HTMLButtonElement | null>(null);
  let confirmedSources = $state<string[]>([]);
  const taskProgress = $derived(section.progress);

  let savingBudget = $state(false);
  let budgetError = $state("");
  async function saveBudget(event: SubmitEvent & { currentTarget: HTMLFormElement }) {
    event.preventDefault();
    const value = Math.round(Number(new FormData(event.currentTarget).get("budget")) * 1024 * 1024);
    if (!Number.isSafeInteger(value) || value < 0 || savingBudget) return;
    savingBudget = true;
    budgetError = "";
    try {
      await api.put("/admin/storage/cache", { maximum_bytes: value });
      await section.load("storage");
    } catch (error) {
      budgetError = error instanceof Error ? error.message : t("admin.unknownError");
    } finally {
      savingBudget = false;
    }
  }
  const cacheRows = [
    "maximum_bytes", "evictable_bytes", "pinned_bytes", "reserved_bytes",
    "legacy_and_unclassified_bytes", "available_bytes", "reserve_free_bytes",
  ] as const;

  function formatStorageSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} ${t('common.bytes')}`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} ${t('common.kilobytes')}`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} ${t('common.megabytes')}`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} ${t('common.gigabytes')}`;
  }
</script>

{#if section.error}
  <p class="rounded-lg border border-destructive/30 bg-destructive/10 p-4 text-destructive" role="alert">{section.error}</p>
{/if}

{#if tab === "storage"}
<div class="space-y-6">
  <div class="bg-[hsl(var(--card))] rounded-lg border border-[hsl(var(--border))] p-6">
    <h2 class="text-lg font-semibold mb-4">{t('admin.storage.title')}</h2>
    {#if storageStatus}
      <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.storage.backend')}</p>
          <p class="text-xl font-bold mt-1 capitalize">{storageStatus.backend}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.storage.tracksStored')}</p>
          <p class="text-xl font-bold mt-1">{storageStatus.total_tracks}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.storage.totalSize')}</p>
          <p class="text-xl font-bold mt-1">{formatStorageSize(storageStatus.total_size_bytes)}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{storageStatus.backend === "s3" ? t('admin.storage.bucket') : t('admin.storage.path')}</p>
          <p class="text-sm font-mono mt-1 truncate" title={storageStatus.storage_path_or_bucket}>{storageStatus.storage_path_or_bucket}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.storage.remoteTracks')}</p>
          <p class="text-xl font-bold mt-1 text-blue-700">{storageStatus.remote_track_count}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.storage.remoteAvailable')}</p>
          <p class="text-xl font-bold mt-1 text-green-700">{storageStatus.remote_available_count}</p>
        </div>
      </div>
      {#if storageStatus.cache}
        <dl class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {#each cacheRows as key}
            <div>
              <dt class="text-xs text-[hsl(var(--muted-foreground))]">{t(`admin.cache.${key}`)}</dt>
              <dd class="font-medium">{formatStorageSize(storageStatus.cache[key])}</dd>
            </div>
          {/each}
        </dl>
        <form class="mt-4 flex flex-wrap items-end gap-3" onsubmit={saveBudget}>
          <label class="grid gap-1 text-sm">{t("admin.cache.limitMiB")}
            <input name="budget" type="number" min="0" step="any" required
              max={storageStatus.cache.configured_ceiling_bytes / 1048576}
              value={storageStatus.cache.maximum_bytes / 1048576}
              class="rounded border bg-background px-2 py-1" />
          </label>
          <button class="rounded border px-3 py-1" type="submit" disabled={savingBudget}>{t("admin.cache.save")}</button>
          {#if budgetError}<span role="alert" class="text-sm text-destructive">{budgetError}</span>{/if}
        </form>
      {/if}
      <div class="mt-4 p-4 bg-[hsl(var(--secondary))] rounded-lg">
        <h3 class="text-sm font-medium mb-2">{t('admin.storage.configuration')}</h3>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">
          {#if storageStatus.backend === "s3"}
            {t('admin.storage.s3Configured')}
            <code class="bg-[hsl(var(--card))] px-1 rounded">S3_ENDPOINT</code>,
            <code class="bg-[hsl(var(--card))] px-1 rounded">S3_REGION</code>,
            <code class="bg-[hsl(var(--card))] px-1 rounded">S3_ACCESS_KEY</code>,
            <code class="bg-[hsl(var(--card))] px-1 rounded">S3_SECRET_KEY</code>,
            <code class="bg-[hsl(var(--card))] px-1 rounded">S3_BUCKET</code>.
          {:else}
            {t('admin.storage.localConfigured')}
            {t('admin.storage.localConfigHint', { var: 'AUDIO_STORAGE_PATH', default: './data/music', s3var: 'STORAGE_BACKEND=s3' })}
          {/if}
        </p>
      </div>
    {:else}
      <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('common.loading')}</p>
    {/if}
  </div>
</div>
{/if}

{#if tab === "integrity"}
<div class="space-y-6">
  <div class="bg-[hsl(var(--card))] rounded-lg border border-[hsl(var(--border))] p-6">
    <div class="flex items-center justify-between mb-4">
      <div>
        <h2 class="text-lg font-semibold">{t('admin.integrity.title')}</h2>
        <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">
          {t('admin.integrity.description')}
        </p>
      </div>
      <Button
        disabled={section.busy}
        onclick={() => section.run("integrity")}
      >
        {storageChecking ? t('admin.integrity.checking') : t('admin.integrity.runCheck')}
      </Button>
    </div>

    {#if storageChecking && taskProgress}
      <div class="mb-4">
        <div class="flex items-center justify-between text-sm text-[hsl(var(--muted-foreground))] mb-1">
          <span>{t('admin.integrity.checking')}</span>
          <span>{taskProgress.processed}{taskProgress.total ? ` / ${taskProgress.total}` : ''}</span>
        </div>
        <div class="w-full bg-[hsl(var(--secondary))] rounded-full h-2">
          <div
            class="bg-[hsl(var(--primary))] h-2 rounded-full transition-all duration-300"
            style="width: {taskProgress.total ? (taskProgress.processed / taskProgress.total * 100) : 50}%"
          ></div>
        </div>
      </div>
    {/if}

    {#if integrityReport}
      <div class="grid grid-cols-3 gap-4 mb-4">
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.integrity.checked')}</p>
          <p class="text-2xl font-bold mt-1">{integrityReport.total_checked}</p>
        </div>
        <div class="bg-green-500/10 border border-green-500/30 rounded-lg p-4 text-center">
          <p class="text-xs text-green-700">{t('admin.integrity.healthy')}</p>
          <p class="text-2xl font-bold mt-1 text-green-700">{integrityReport.healthy}</p>
        </div>
        <div class="rounded-lg p-4 text-center {integrityReport.missing.length > 0 ? 'bg-red-500/10 border border-red-500/30' : 'bg-[hsl(var(--secondary))]'}">
          <p class="text-xs {integrityReport.missing.length > 0 ? 'text-destructive' : 'text-[hsl(var(--muted-foreground))]'}">{t('admin.integrity.missing')}</p>
          <p class="text-2xl font-bold mt-1 {integrityReport.missing.length > 0 ? 'text-destructive' : ''}">{integrityReport.missing.length}</p>
        </div>
      </div>

      {#if integrityReport.missing.length > 0}
        <div class="mt-4">
          <h3 class="text-sm font-medium mb-2 text-destructive">{t('admin.integrity.missingTracks')}</h3>
          <button class="mb-3 underline text-sm" onclick={() => onSyncTab()}>
            {t('admin.integrity.reconcile')}
          </button>
          <div class="space-y-2 max-h-64 overflow-y-auto">
            {#each integrityReport.missing as m (m.track_id)}
              <div class="flex items-center gap-3 p-2 bg-red-500/5 rounded text-sm">
                <span class="font-medium truncate flex-1">{m.title}</span>
                <span class="text-xs text-[hsl(var(--muted-foreground))] font-mono truncate max-w-[200px]">{m.file_path}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if integrityReport.errors.length > 0}
        <div class="mt-4">
          <h3 class="text-sm font-medium mb-2 text-amber-700">{t('admin.integrity.errors')}</h3>
          <div class="space-y-1 max-h-40 overflow-y-auto">
            {#each integrityReport.errors as err, index (index)}
              <p class="text-xs text-amber-700 font-mono">{err}</p>
            {/each}
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>
{/if}

{#if tab === "sync"}
<div class="space-y-6">
  <div class="bg-[hsl(var(--card))] rounded-lg border border-[hsl(var(--border))] p-6">
    <div class="flex items-center justify-between mb-4">
      <div>
        <h2 class="text-lg font-semibold">{t('admin.sync.title')}</h2>
        <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">
          {t('admin.sync.description')}
        </p>
      </div>
      <Button
        disabled={section.busy}
        onclick={() => section.run("sync")}
      >
        {storageSyncing ? t('admin.sync.syncing') : t('admin.sync.runSync')}
      </Button>
    </div>

    {#if storageSyncing && taskProgress}
      <div class="mb-4">
        <div class="flex items-center justify-between text-sm text-[hsl(var(--muted-foreground))] mb-1">
          <span>{t('admin.sync.syncing')}</span>
          <span>{taskProgress.processed}{taskProgress.total ? ` / ${taskProgress.total}` : ''}</span>
        </div>
        <div class="w-full bg-[hsl(var(--secondary))] rounded-full h-2">
          <div
            class="bg-[hsl(var(--primary))] h-2 rounded-full transition-all duration-300"
            style="width: {taskProgress.total ? (taskProgress.processed / taskProgress.total * 100) : 50}%"
          ></div>
        </div>
      </div>
    {/if}

    {#if syncReport}
      <div class="grid grid-cols-2 sm:grid-cols-5 gap-4 mb-4">
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.sync.scanned')}</p>
          <p class="text-2xl font-bold mt-1">{syncReport.scanned}</p>
        </div>
        <div class="bg-green-500/10 border border-green-500/30 rounded-lg p-4 text-center">
          <p class="text-xs text-green-700">{t('admin.sync.imported')}</p>
          <p class="text-2xl font-bold mt-1 text-green-700">{syncReport.imported}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.sync.alreadyReferenced')}</p>
          <p class="text-2xl font-bold mt-1">{syncReport.skipped}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.sync.moved')}</p>
          <p class="text-2xl font-bold mt-1">{syncReport.moved}</p>
        </div>
        <div class="bg-[hsl(var(--secondary))] rounded-lg p-4 text-center">
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('admin.sync.removed')}</p>
          <p class="text-2xl font-bold mt-1">{syncReport.removed}</p>
        </div>
      </div>

      {#if syncReport.empty_sources?.length}
        <p class="text-sm mb-2">{t('admin.sync.emptyWarning', { sources: syncReport.empty_sources.join(', ') })}</p>
        <AlertDialog.Root bind:open={confirming} onOpenChange={open => {
          if (open) confirmedSources = [...(syncReport?.empty_sources ?? [])];
        }}>
          <AlertDialog.Trigger bind:ref={cleanupTrigger} class="text-sm text-destructive underline" disabled={section.busy}>
            {t('admin.sync.cleanEmpty')}
          </AlertDialog.Trigger>
          <AlertDialog.Portal>
            <AlertDialog.Overlay class="ui-overlay z-[201]" />
            <AlertDialog.Content onCloseAutoFocus={event => {
              event.preventDefault();
              cleanupTrigger?.focus();
            }} class="ui-dialog z-[202] space-y-4">
              <AlertDialog.Title class="text-lg font-semibold">{t('admin.sync.cleanEmpty')}</AlertDialog.Title>
              <AlertDialog.Description class="text-sm text-muted-foreground">
                {t('admin.sync.confirmEmpty', { sources: confirmedSources.join(', ') })}
              </AlertDialog.Description>
              <div class="flex flex-wrap justify-end gap-2">
                <AlertDialog.Cancel class={buttonVariants({ variant: "outline" })}>{t('common.cancel')}</AlertDialog.Cancel>
                <AlertDialog.Action class={buttonVariants({ variant: "destructive" })} onclick={() => {
                  confirming = false;
                  if (confirmedSources.length) void section.run("sync", confirmedSources);
                }}>{t('admin.sync.cleanEmpty')}</AlertDialog.Action>
              </div>
            </AlertDialog.Content>
          </AlertDialog.Portal>
        </AlertDialog.Root>
      {/if}

      {#if syncReport.errors.length > 0}
        <div class="mt-4">
          <h3 class="text-sm font-medium mb-2 text-amber-700">{t('admin.sync.importErrors')}</h3>
          <div class="space-y-1 max-h-40 overflow-y-auto">
            {#each syncReport.errors as err, index (index)}
              <p class="text-xs text-amber-700 font-mono">{err}</p>
            {/each}
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>
{/if}
