<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Track } from "$lib/types";
  import TrackList from "$lib/components/TrackList.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  let tracks: Track[] = $state([]);
  let loading = $state(true);

  onMount(async () => {
    if (!auth.isAuthenticated) { loading = false; return; }
    try {
      const res = await api.get<{ data: Track[]; total: number }>("/favorites?per_page=200");
      tracks = res.data ?? [];
    } catch (e) { console.error('Failed to load favorites:', e); } finally { loading = false; }
  });
</script>

<svelte:head><title>Favorites — SoundTime</title></svelte:head>

<div class="space-y-6">
  <div class="flex items-end gap-6">
    <div class="w-48 h-48 rounded-lg bg-gradient-to-br from-pink-500/30 to-purple-500/30 flex items-center justify-center text-6xl shadow-xl">❤️</div>
    <div>
      <p class="text-xs uppercase tracking-wider text-[hsl(var(--muted-foreground))]">Playlist</p>
      <h1 class="text-4xl font-bold">{t('favorites.title')}</h1>
      <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">{tracks.length} {t('playlists.tracks')}</p>
    </div>
  </div>

  {#if !auth.isAuthenticated}
    <p class="text-[hsl(var(--muted-foreground))] text-center py-10">{t('nav.signIn')}</p>
  {:else if loading}
    <div class="flex justify-center py-10">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else if tracks.length > 0}
    <TrackList {tracks} />
  {:else}
    <p class="text-[hsl(var(--muted-foreground))] text-center py-10">{t('favorites.empty')}</p>
  {/if}
</div>
