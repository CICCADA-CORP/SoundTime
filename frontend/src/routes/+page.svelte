<script lang="ts">
  import { onMount } from "svelte";
  import type {
    Track,
    Album,
    Artist,
    EditorialPlaylist,
    HistoryEntry,
    StatsOverview,
  } from "$lib/types";
  import { homeApi } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { Music, Disc3, Users, Clock, RefreshCw } from "lucide-svelte";

  import SectionCarousel from "$lib/components/SectionCarousel.svelte";
  import TrackCard from "$lib/components/TrackCard.svelte";
  import QuickPlayCard from "$lib/components/QuickPlayCard.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import StatCard from "$lib/components/StatCard.svelte";
  import SkeletonCard from "$lib/components/SkeletonCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";

  // ─── State ───────────────────────────────────────────────────────────
  let loading = $state(true);
  let popularTracks: Track[] = $state([]);
  let recentAlbums: Album[] = $state([]);
  let topArtists: Artist[] = $state([]);
  let editorialPlaylists: EditorialPlaylist[] = $state([]);
  let randomTracks: Track[] = $state([]);
  let stats: StatsOverview | null = $state(null);
  let recentHistory: HistoryEntry[] = $state([]);
  let selectedEditorial: EditorialPlaylist | null = $state(null);
  let refreshingRandom = $state(false);

  // ─── Derived ─────────────────────────────────────────────────────────
  let greeting = $derived.by(() => {
    const hour = new Date().getHours();
    if (hour >= 5 && hour < 12) return t("home.greeting.morning");
    if (hour >= 12 && hour < 18) return t("home.greeting.afternoon");
    return t("home.greeting.evening");
  });

  let hasContent = $derived(
    popularTracks.length > 0 ||
      recentAlbums.length > 0 ||
      topArtists.length > 0 ||
      editorialPlaylists.length > 0 ||
      randomTracks.length > 0,
  );

  let isLoggedIn = $derived(
    typeof window !== "undefined" &&
      !!localStorage.getItem("soundtime_access_token"),
  );

  // ─── Data Fetching ───────────────────────────────────────────────────
  onMount(async () => {
    try {
      const [popular, albums, artists, editorial, random, overview] =
        await Promise.all([
          homeApi.popularTracks(12),
          homeApi.recentAlbums(12),
          homeApi.topArtists(12),
          homeApi.editorialPlaylists(),
          homeApi.randomTracks(8),
          homeApi.statsOverview(),
        ]);

      popularTracks = popular.data ?? [];
      recentAlbums = albums.data ?? [];
      topArtists = artists.data ?? [];
      editorialPlaylists = editorial ?? [];
      randomTracks = random ?? [];
      stats = overview;

      // History requires auth — may fail silently
      try {
        recentHistory = await homeApi.recentHistory(6);
      } catch {}
    } catch {
      // Server unavailable
    } finally {
      loading = false;
    }
  });

  // ─── Actions ─────────────────────────────────────────────────────────
  async function refreshRandom() {
    refreshingRandom = true;
    try {
      randomTracks = await homeApi.randomTracks(8);
    } catch {}
    finally {
      refreshingRandom = false;
    }
  }

  function openEditorial(pl: EditorialPlaylist) {
    selectedEditorial = pl;
  }

  function closeEditorial() {
    selectedEditorial = null;
  }
</script>

<svelte:head>
  <title>SoundTime — Home</title>
</svelte:head>

{#if loading}
  <!-- Loading skeleton -->
  <div class="space-y-8">
    <div class="animate-pulse">
      <div class="h-8 bg-[hsl(var(--secondary))] rounded w-64 mb-2"></div>
      <div class="h-4 bg-[hsl(var(--secondary))] rounded w-96"></div>
    </div>
    <div class="flex gap-4 overflow-hidden">
      {#each Array(8) as _}
        <SkeletonCard />
      {/each}
    </div>
  </div>
{:else}
  <div class="space-y-10">
    <!-- ─── Greeting ────────────────────────────────────────────────── -->
    <section class="relative overflow-hidden rounded-2xl bg-gradient-to-br from-[hsl(var(--primary)/0.15)] via-[hsl(var(--primary)/0.05)] to-transparent border border-[hsl(var(--border))] p-8 md:p-12">
      <div class="absolute -top-20 -right-20 w-64 h-64 bg-[hsl(var(--primary)/0.08)] rounded-full blur-3xl"></div>
      <div class="absolute -bottom-16 -left-16 w-48 h-48 bg-purple-500/5 rounded-full blur-3xl"></div>
      <div class="relative">
        <h1 class="text-3xl md:text-4xl font-bold mb-2">{greeting}</h1>
        <p class="text-[hsl(var(--muted-foreground))] text-lg max-w-xl">{t('home.subtitle')}</p>
      </div>
    </section>

    <!-- ─── Recently Played (Quick Play) ────────────────────────────── -->
    {#if isLoggedIn && recentHistory.length > 0}
      <section>
        <h2 class="text-xl font-bold mb-4">{t('home.recentlyPlayed')}</h2>
        <div class="grid grid-cols-2 lg:grid-cols-3 gap-3">
          {#each recentHistory as entry}
            <QuickPlayCard track={entry.track} />
          {/each}
        </div>
      </section>
    {/if}

    <!-- ─── Editorial Playlists ─────────────────────────────────────── -->
    {#if editorialPlaylists.length > 0}
      <SectionCarousel title={t('home.editorialPicks')}>
        {#snippet children()}
          {#each editorialPlaylists as pl}
            <button
              class="flex-shrink-0 w-48 group cursor-pointer text-left snap-start"
              onclick={() => openEditorial(pl)}
            >
              <div class="relative aspect-square rounded-xl overflow-hidden mb-2 shadow-lg ring-1 ring-black/5">
                {#if pl.cover_url}
                  <img
                    src={pl.cover_url}
                    alt={pl.name}
                    loading="lazy"
                    class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                  />
                {:else}
                  <div class="w-full h-full bg-gradient-to-br from-[hsl(var(--primary))] to-purple-700 flex items-center justify-center">
                    <Music class="w-12 h-12 text-white/60" />
                  </div>
                {/if}
                <div class="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity flex items-end p-3">
                  <div class="flex items-center gap-2 text-white">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>
                    <span class="text-xs font-medium">{pl.track_count} {t('explore.tracks')}</span>
                  </div>
                </div>
              </div>
              <h3 class="font-semibold text-sm truncate group-hover:text-[hsl(var(--primary))] transition-colors">
                {pl.name}
              </h3>
              {#if pl.description}
                <p class="text-xs text-[hsl(var(--muted-foreground))] line-clamp-1 mt-0.5">{pl.description}</p>
              {/if}
            </button>
          {/each}
        {/snippet}
      </SectionCarousel>
    {/if}

    <!-- ─── Editorial Detail Overlay ────────────────────────────────── -->
    {#if selectedEditorial}
      <section class="bg-[hsl(var(--card))] rounded-xl border border-[hsl(var(--border))] p-6 shadow-sm">
        <div class="flex items-start justify-between mb-5">
          <div class="flex gap-4">
            {#if selectedEditorial.cover_url}
              <img src={selectedEditorial.cover_url} alt={selectedEditorial.name} class="w-20 h-20 rounded-lg object-cover shadow" />
            {:else}
              <div class="w-20 h-20 rounded-lg bg-gradient-to-br from-[hsl(var(--primary))] to-purple-700 flex items-center justify-center shadow">
                <Music class="w-8 h-8 text-white/60" />
              </div>
            {/if}
            <div>
              <h2 class="text-xl font-bold">{selectedEditorial.name}</h2>
              {#if selectedEditorial.description}
                <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">{selectedEditorial.description}</p>
              {/if}
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{selectedEditorial.track_count} {t('explore.tracks')}</p>
            </div>
          </div>
          <button
            class="text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))] transition p-1.5 rounded-md hover:bg-[hsl(var(--secondary))]"
            onclick={closeEditorial}
            aria-label="Close"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
          </button>
        </div>
        <TrackList tracks={selectedEditorial.tracks} />
      </section>
    {/if}

    <!-- ─── Popular Tracks ──────────────────────────────────────────── -->
    {#if popularTracks.length > 0}
      <SectionCarousel title={t('home.popularNow')} href="/tracks">
        {#snippet children()}
          {#each popularTracks as track, i}
            <TrackCard {track} tracks={popularTracks} index={i} />
          {/each}
        {/snippet}
      </SectionCarousel>
    {/if}

    <!-- ─── Fresh Releases ──────────────────────────────────────────── -->
    {#if recentAlbums.length > 0}
      <SectionCarousel title={t('home.freshReleases')} href="/albums">
        {#snippet children()}
          {#each recentAlbums as album}
            <div class="flex-shrink-0 w-44">
              <AlbumCard {album} />
            </div>
          {/each}
        {/snippet}
      </SectionCarousel>
    {/if}

    <!-- ─── Random Discovery ────────────────────────────────────────── -->
    {#if randomTracks.length > 0}
      <section>
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-xl font-bold">{t('home.randomDiscovery')}</h2>
          <button
            class="flex items-center gap-1.5 text-sm text-[hsl(var(--primary))] hover:underline font-medium disabled:opacity-50"
            onclick={refreshRandom}
            disabled={refreshingRandom}
          >
            <RefreshCw class="w-3.5 h-3.5 {refreshingRandom ? 'animate-spin' : ''}" />
            {t('home.refreshRandom')}
          </button>
        </div>
        <SectionCarousel title="">
          {#snippet children()}
            {#each randomTracks as track, i}
              <TrackCard {track} tracks={randomTracks} index={i} />
            {/each}
          {/snippet}
        </SectionCarousel>
      </section>
    {/if}

    <!-- ─── Top Artists ─────────────────────────────────────────────── -->
    {#if topArtists.length > 0}
      <SectionCarousel title={t('home.topArtists')} href="/artists">
        {#snippet children()}
          {#each topArtists as artist}
            <div class="flex-shrink-0 w-36">
              <ArtistCard {artist} />
            </div>
          {/each}
        {/snippet}
      </SectionCarousel>
    {/if}

    <!-- ─── Network Stats ───────────────────────────────────────────── -->
    {#if stats}
      <section>
        <h2 class="text-xl font-bold mb-4">{t('home.networkStats')}</h2>
        <div class="grid grid-cols-2 lg:grid-cols-4 gap-3">
          <StatCard value={stats.total_tracks} label={t('stats.totalTracks')}>
            {#snippet icon()}<Music class="w-5 h-5" />{/snippet}
          </StatCard>
          <StatCard value={stats.total_albums} label={t('stats.totalAlbums')}>
            {#snippet icon()}<Disc3 class="w-5 h-5" />{/snippet}
          </StatCard>
          <StatCard value={stats.total_artists} label={t('stats.totalArtists')}>
            {#snippet icon()}<Users class="w-5 h-5" />{/snippet}
          </StatCard>
          <StatCard value={Math.round(stats.total_duration_secs / 3600)} label={t('stats.totalDuration')}>
            {#snippet icon()}<Clock class="w-5 h-5" />{/snippet}
          </StatCard>
        </div>
      </section>
    {/if}

    <!-- ─── Empty State ─────────────────────────────────────────────── -->
    {#if !hasContent}
      <section class="text-center py-16">
        <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center">
          <Music class="w-8 h-8 text-[hsl(var(--muted-foreground))]" />
        </div>
        <h2 class="text-xl font-semibold mb-2">{t('home.noMusic')}</h2>
        <p class="text-[hsl(var(--muted-foreground))] mb-4">Upload your first tracks to get started.</p>
        <a
          href="/upload"
          class="inline-block px-6 py-2 bg-[hsl(var(--primary))] text-white rounded-full text-sm font-medium hover:opacity-90 transition"
        >
          {t('home.uploadFirst')}
        </a>
      </section>
    {/if}
  </div>
{/if}
