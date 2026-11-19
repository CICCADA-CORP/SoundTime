<script lang="ts">
  import { onMount } from "svelte";
  import { lastfmApi } from "$lib/api";
  import type { LastfmStatus } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  let lastfmStatus = $state<LastfmStatus | null>(null);
  let lastfmLoading = $state(true);
  let lastfmError = $state("");
  const lastfmConfigured = $derived(lastfmStatus?.connected);

  onMount(async () => {
    const url = new URL(window.location.href);
    const token = url.searchParams.get("token");
    try {
      if (token) {
        await lastfmApi.callback(token);
        url.searchParams.delete("token");
        window.history.replaceState(window.history.state, "", url);
      }
      lastfmStatus = await lastfmApi.status();
    } catch (cause) {
      lastfmError = cause instanceof Error ? cause.message : t("settings.lastfmError");
    } finally {
      lastfmLoading = false;
    }
  });

  async function connectLastfm() {
    lastfmLoading = true;
    lastfmError = "";
    try {
      const res = await lastfmApi.connect();
      window.location.href = res.auth_url;
    } catch (e: unknown) {
      lastfmError = e instanceof Error ? e.message : t("settings.lastfmError");
      lastfmLoading = false;
    }
  }

  async function disconnectLastfm() {
    lastfmLoading = true;
    lastfmError = "";
    try {
      await lastfmApi.disconnect();
      lastfmStatus = null;
    } catch (e: unknown) {
      lastfmError = e instanceof Error ? e.message : t("settings.lastfmError");
    } finally {
      lastfmLoading = false;
    }
  }

  async function toggleLastfmScrobble() {
    if (!lastfmStatus || lastfmLoading) return;
    lastfmError = "";
    lastfmLoading = true;
    const newVal = !lastfmStatus.scrobble_enabled;
    try {
      await lastfmApi.toggleScrobble(newVal);
      lastfmStatus = { ...lastfmStatus, scrobble_enabled: newVal };
    } catch (e: unknown) {
      lastfmError = e instanceof Error ? e.message : t("settings.lastfmError");
    } finally {
      lastfmLoading = false;
    }
  }

</script>

<!-- Last.fm -->
<section class="space-y-4 border-b border-border py-6">
  <h2 class="text-lg font-semibold">{t('settings.lastfm')}</h2>
  {#if lastfmError}
    <p role="alert" class="text-sm text-destructive">{lastfmError}</p>
  {/if}

  {#if lastfmConfigured && lastfmStatus}
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-medium">{t('settings.lastfmConnectedAs')}</p>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">{lastfmStatus.username}</p>
      </div>
      <button
        class="px-3 py-1.5 text-sm bg-red-500/20 text-destructive rounded-lg hover:bg-red-500/30 transition disabled:opacity-50"
        onclick={disconnectLastfm}
        disabled={lastfmLoading}
      >
        {lastfmLoading ? t('common.loading') : t('settings.lastfmDisconnect')}
      </button>
    </div>
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-medium">{t('settings.lastfmScrobbling')}</p>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('settings.lastfmScrobblingHint')}</p>
      </div>
      <button
        class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors {lastfmStatus.scrobble_enabled ? 'bg-[hsl(var(--primary))]' : 'bg-[hsl(var(--secondary))]'}"
        disabled={lastfmLoading}
        onclick={toggleLastfmScrobble}
        role="switch"
        aria-checked={lastfmStatus.scrobble_enabled}
        aria-label={t('settings.lastfmScrobbling')}
      >
        <span
          class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform {lastfmStatus.scrobble_enabled ? 'translate-x-6' : 'translate-x-1'}"
        ></span>
      </button>
    </div>
  {:else}
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-medium">{t('settings.lastfmConnect')}</p>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('settings.lastfmConnectHint')}</p>
      </div>
      <button
        class="px-4 py-2 bg-primary text-primary-foreground rounded-lg text-sm font-medium disabled:opacity-50"
        onclick={connectLastfm}
        disabled={lastfmLoading}
      >
        {lastfmLoading ? t('common.loading') : t('settings.lastfmConnect')}
      </button>
    </div>
  {/if}
</section>
