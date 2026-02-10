<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { formatDuration, formatDate } from "$lib/utils";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { t } from "$lib/i18n/index.svelte";

  interface HistoryEntry {
    id: string;
    track_id: string;
    listened_at: string;
    track?: {
      id: string;
      title: string;
      duration_secs: number;
      artist_name?: string;
    };
  }

  const auth = getAuthStore();
  const player = getPlayerStore();
  let history: HistoryEntry[] = [];
  let loading = true;

  onMount(async () => {
    if (!auth.isAuthenticated) { loading = false; return; }
    try {
      const res = await api.get<{ data: HistoryEntry[] }>("/history");
      history = res.data ?? [];
    } catch (e) { console.error('Failed to load history:', e); } finally { loading = false; }
  });
</script>

<svelte:head><title>History — SoundTime</title></svelte:head>

<div class="space-y-6">
  <h1 class="text-2xl font-bold">{t('history.title')}</h1>

  {#if !auth.isAuthenticated}
    <p class="text-[hsl(var(--muted-foreground))] text-center py-10">Sign in to see your history.</p>
  {:else if loading}
    <div class="flex justify-center py-10">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else if history.length > 0}
    <div class="space-y-1">
      {#each history as entry}
        <button class="w-full flex items-center gap-4 px-4 py-3 rounded-lg hover:bg-[hsl(var(--secondary))] transition text-left" onclick={() => entry.track && player.play(entry.track as any)}>
          <div class="flex-1 min-w-0">
            <p class="text-sm font-medium truncate">{entry.track?.title ?? "Unknown"}</p>
            <p class="text-xs text-[hsl(var(--muted-foreground))]">{entry.track?.artist_name ?? ""}</p>
          </div>
          <span class="text-xs text-[hsl(var(--muted-foreground))]">{formatDate(entry.listened_at)}</span>
          <span class="text-xs text-[hsl(var(--muted-foreground))] w-12 text-right">{formatDuration(entry.track?.duration_secs ?? 0)}</span>
        </button>
      {/each}
    </div>
  {:else}
    <p class="text-[hsl(var(--muted-foreground))] text-center py-10">No listening history yet.</p>
  {/if}
</div>
