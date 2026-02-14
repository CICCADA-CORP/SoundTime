<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { Track, Album, Artist, EditorialPlaylist, StatsOverview, PaginatedResponse } from "$lib/types";
  import { homeApi } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { Music, Disc3, Users, Radio, Shuffle, Play, X } from "lucide-svelte";
  import SectionCarousel from "$lib/components/SectionCarousel.svelte";
  import HeroBanner from "$lib/components/HeroBanner.svelte";
  import GenreChip from "$lib/components/GenreChip.svelte";
  import MoodCard from "$lib/components/MoodCard.svelte";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import ArtistCard from "$lib/components/ArtistCard.svelte";
  import StatCard from "$lib/components/StatCard.svelte";
  import SkeletonCard from "$lib/components/SkeletonCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";

  const player = getPlayerStore();
  const queue = getQueueStore();

  // ─── State ─────────────────────────────────────────────────────────
  let loading = $state(true);
  let tracks: Track[] = $state([]);
  let albums: Album[] = $state([]);
  let artists: Artist[] = $state([]);
  let editorialPlaylists: EditorialPlaylist[] = $state([]);
  let genres: string[] = $state([]);
  let stats: StatsOverview | null = $state(null);
  let selectedEditorial: EditorialPlaylist | null = $state(null);

  // Hero banner rotation
  let heroIndex = $state(0);
  let heroInterval: ReturnType<typeof setInterval> | undefined;

  // Genre filter
  let activeGenre: string | null = $state(null);
  let genreTracks: Track[] = $state([]);
  let genreLoading = $state(false);

  // Surprise Me
  let surpriseLoading = $state(false);

  // ─── Computed ──────────────────────────────────────────────────────
  let heroItems = $derived<Array<{ item: EditorialPlaylist | Album; type: "editorial" | "album" }>>(
    [
      ...editorialPlaylists.map((pl) => ({ item: pl as EditorialPlaylist | Album, type: "editorial" as const })),
      ...albums.slice(0, 3).map((al) => ({ item: al as EditorialPlaylist | Album, type: "album" as const })),
    ]
  );

  // ─── Data Fetching ────────────────────────────────────────────────
  onMount(async () => {
    try {
      const [popularRes, albumsRes, artistsRes, editorial, genreList, statsRes] = await Promise.all([
        homeApi.popularTracks(10),
        homeApi.recentAlbums(12),
        homeApi.topArtists(12),
        homeApi.editorialPlaylists(),
        homeApi.genres(),
        homeApi.statsOverview(),
      ]);
      tracks = popularRes.data ?? [];
      albums = albumsRes.data ?? [];
      artists = artistsRes.data ?? [];
      editorialPlaylists = editorial ?? [];
      genres = genreList ?? [];
      stats = statsRes ?? null;

      // Start hero rotation if there are items
      if (heroItems.length > 1) {
        heroInterval = setInterval(() => {
          heroIndex = (heroIndex + 1) % heroItems.length;
        }, 8000);
      }
    } catch (e) {
      console.error('Failed to load explore data:', e);
    } finally {
      loading = false;
    }
  });

  onDestroy(() => {
    if (heroInterval) clearInterval(heroInterval);
  });

  // ─── Actions ──────────────────────────────────────────────────────
  function setHeroIndex(index: number) {
    heroIndex = index;
    // Reset timer on manual navigation
    if (heroInterval) clearInterval(heroInterval);
    if (heroItems.length > 1) {
      heroInterval = setInterval(() => {
        heroIndex = (heroIndex + 1) % heroItems.length;
      }, 8000);
    }
  }

  function handleHeroClick(heroItem: { item: EditorialPlaylist | Album; type: "editorial" | "album" }) {
    if (heroItem.type === "editorial") {
      openEditorial(heroItem.item as EditorialPlaylist);
    }
  }

  async function selectGenre(genre: string | null) {
    activeGenre = genre;
    genreTracks = [];
    if (!genre) return;
    genreLoading = true;
    try {
      const res = await homeApi.genreTracks(genre, 10);
      genreTracks = res.data ?? [];
    } catch (e) {
      console.error('Failed to load genre tracks:', e);
    } finally {
      genreLoading = false;
    }
  }

  function openEditorial(pl: EditorialPlaylist) {
    selectedEditorial = pl;
  }

  function closeEditorial() {
    selectedEditorial = null;
  }

  async function handleSurpriseMe() {
    surpriseLoading = true;
    try {
      const randomTracks = await homeApi.randomTracks(20);
      if (randomTracks.length > 0) {
        queue.playQueue(randomTracks);
      }
    } catch (e) {
      console.error('Failed to load random tracks:', e);
    } finally {
      surpriseLoading = false;
    }
  }
</script>

<svelte:head><title>{t('explore.title')} — SoundTime</title></svelte:head>

{#if loading}
  <!-- Loading skeleton -->
  <div class="space-y-10">
    <!-- Hero skeleton -->
    <div class="h-56 md:h-72 rounded-2xl bg-[hsl(var(--secondary))] animate-pulse"></div>

    <!-- Genre chips skeleton -->
    <div class="flex gap-2 flex-wrap">
      {#each Array(8) as _}
        <div class="h-8 w-20 rounded-full bg-[hsl(var(--secondary))] animate-pulse"></div>
      {/each}
    </div>

    <!-- Cards skeleton -->
    <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-3">
      {#each Array(6) as _}
        <div class="h-28 rounded-xl bg-[hsl(var(--secondary))] animate-pulse"></div>
      {/each}
    </div>

    <!-- Carousel skeleton -->
    <div class="flex gap-4 overflow-hidden">
      {#each Array(6) as _}
        <SkeletonCard />
      {/each}
    </div>

    <!-- Spinner -->
    <div class="flex justify-center py-8">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  </div>
{:else if tracks.length === 0 && albums.length === 0 && artists.length === 0 && editorialPlaylists.length === 0}
  <!-- Empty state -->
  <div class="text-center py-16">
    <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center">
      <Music class="w-8 h-8 text-[hsl(var(--muted-foreground))]" />
    </div>
    <p class="text-[hsl(var(--muted-foreground))]">{t('explore.empty')}</p>
  </div>
{:else}
  <div class="space-y-10">

    <!-- ─── 1. Dynamic Hero Banner ─────────────────────────────────── -->
    {#if heroItems.length > 0}
      <div class="relative">
        <HeroBanner
          item={heroItems[heroIndex].item}
          type={heroItems[heroIndex].type}
          onclick={() => handleHeroClick(heroItems[heroIndex])}
        />
        <!-- Dot navigation -->
        {#if heroItems.length > 1}
          <div class="absolute bottom-4 left-1/2 -translate-x-1/2 flex gap-2 z-10">
            {#each heroItems as _, i}
              <button
                class="w-2 h-2 rounded-full transition-all {i === heroIndex
                  ? 'bg-white w-6'
                  : 'bg-white/50 hover:bg-white/75'}"
                onclick={() => setHeroIndex(i)}
                aria-label="Go to slide {i + 1}"
              ></button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- ─── 2. Genre Chips ─────────────────────────────────────────── -->
    {#if genres.length > 0}
      <section>
        <h2 class="text-xl font-bold mb-4">{t('explore.genres')}</h2>
        <div class="flex flex-wrap gap-2">
          <GenreChip
            genre="All"
            active={activeGenre === null}
            onclick={() => selectGenre(null)}
          />
          {#each genres as genre}
            <GenreChip
              {genre}
              active={activeGenre === genre}
              onclick={() => selectGenre(genre)}
            />
          {/each}
        </div>

        <!-- Genre filtered tracks -->
        {#if activeGenre}
          <div class="mt-4">
            {#if genreLoading}
              <div class="flex justify-center py-6">
                <div class="w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
              </div>
            {:else if genreTracks.length > 0}
              <TrackList tracks={genreTracks} />
            {:else}
              <p class="text-sm text-[hsl(var(--muted-foreground))] py-4">{t('explore.noTracks')}</p>
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    <!-- ─── 3. Mood Cards ──────────────────────────────────────────── -->
    <section>
      <h2 class="text-xl font-bold mb-4">{t('explore.moods')}</h2>
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-3">
        <MoodCard
          mood={t('mood.focus')}
          gradient="bg-gradient-to-br from-blue-600 to-cyan-500"
          emoji="🎯"
        />
        <MoodCard
          mood={t('mood.party')}
          gradient="bg-gradient-to-br from-pink-500 to-orange-400"
          emoji="🎉"
        />
        <MoodCard
          mood={t('mood.chill')}
          gradient="bg-gradient-to-br from-green-500 to-teal-400"
          emoji="🌿"
        />
        <MoodCard
          mood={t('mood.love')}
          gradient="bg-gradient-to-br from-rose-500 to-pink-400"
          emoji="💕"
        />
        <MoodCard
          mood={t('mood.energy')}
          gradient="bg-gradient-to-br from-amber-500 to-red-500"
          emoji="⚡"
        />
        <MoodCard
          mood={t('mood.melancholy')}
          gradient="bg-gradient-to-br from-indigo-600 to-purple-500"
          emoji="🌧️"
        />
      </div>
    </section>

    <!-- ─── 4. Curated Playlists (Editorial) ───────────────────────── -->
    {#if editorialPlaylists.length > 0}
      <section>
        <div class="mb-4">
          <h2 class="text-xl font-bold">{t('explore.editorialPlaylists')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('explore.curatedByAi')}</p>
        </div>
        <SectionCarousel title={t('explore.editorialPlaylists')}>
          {#snippet children()}
            {#each editorialPlaylists as pl}
              <button
                class="flex-shrink-0 w-48 group cursor-pointer text-left"
                onclick={() => openEditorial(pl)}
              >
                <div class="relative aspect-square rounded-xl overflow-hidden mb-2 shadow-lg ring-1 ring-black/5">
                  {#if pl.cover_url}
                    <img
                      src={pl.cover_url}
                      alt={pl.name}
                      class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                    />
                  {:else}
                    <div class="w-full h-full bg-gradient-to-br from-[hsl(var(--primary))] to-purple-700 flex items-center justify-center">
                      <Music class="w-12 h-12 text-white/60" />
                    </div>
                  {/if}
                  <div class="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity flex items-end p-3">
                    <div class="flex items-center gap-2 text-white">
                      <Play class="w-5 h-5" fill="currentColor" />
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
      </section>
    {/if}

    <!-- ─── Editorial Detail Overlay ───────────────────────────────── -->
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
            <X class="w-5 h-5" />
          </button>
        </div>
        <TrackList tracks={selectedEditorial.tracks} />
      </section>
    {/if}

    <!-- ─── 5. Trending Top-10 ─────────────────────────────────────── -->
    {#if tracks.length > 0}
      <section>
        <h2 class="text-xl font-bold mb-4">{t('explore.trending')}</h2>
        <div class="space-y-1">
          {#each tracks.slice(0, 10) as track, i}
            <button
              class="w-full flex items-center gap-4 p-3 rounded-lg hover:bg-[hsl(var(--secondary))] transition group text-left"
              onclick={() => player.play(track)}
            >
              <span class="text-2xl font-bold text-[hsl(var(--muted-foreground))] w-8 text-right">{i + 1}</span>
              <div class="w-10 h-10 rounded bg-[hsl(var(--secondary))] overflow-hidden flex-shrink-0">
                {#if track.cover_url}
                  <img src={track.cover_url} alt={track.title} class="w-full h-full object-cover" />
                {:else}
                  <div class="w-full h-full flex items-center justify-center text-sm">🎵</div>
                {/if}
              </div>
              <div class="flex-1 min-w-0">
                <p class="text-sm font-medium truncate">{track.title}</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))] truncate">{track.artist_name ?? t('explore.unknownArtist')}</p>
              </div>
              <span class="text-xs text-[hsl(var(--muted-foreground))] hidden sm:inline">{track.play_count} {t('track.plays').toLowerCase()}</span>
              <div class="w-8 h-8 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center opacity-0 group-hover:opacity-100 transition">
                <Play class="w-4 h-4 ml-0.5" fill="currentColor" />
              </div>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <!-- ─── 6. New This Week (Albums Carousel) ─────────────────────── -->
    {#if albums.length > 0}
      <SectionCarousel title={t('explore.newThisWeek')} href="/albums">
        {#snippet children()}
          {#each albums as album}
            <div class="flex-shrink-0 w-44">
              <AlbumCard {album} />
            </div>
          {/each}
        {/snippet}
      </SectionCarousel>
    {/if}

    <!-- ─── 7. Artists to Discover ─────────────────────────────────── -->
    {#if artists.length > 0}
      <SectionCarousel title={t('explore.discoverArtists')} href="/artists">
        {#snippet children()}
          {#each artists as artist}
            <div class="flex-shrink-0 w-36">
              <ArtistCard {artist} />
            </div>
          {/each}
        {/snippet}
      </SectionCarousel>
    {/if}

    <!-- ─── 8. From the Network (P2P Stats) ────────────────────────── -->
    {#if stats}
      <section>
        <h2 class="text-xl font-bold mb-4">{t('explore.fromTheNetwork')}</h2>
        <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
          <StatCard value={stats.total_tracks} label={t('stats.totalTracks')}>
            {#snippet icon()}<Music class="w-5 h-5" />{/snippet}
          </StatCard>
          <StatCard value={stats.total_albums} label={t('stats.totalAlbums')}>
            {#snippet icon()}<Disc3 class="w-5 h-5" />{/snippet}
          </StatCard>
          <StatCard value={stats.total_artists} label={t('stats.totalArtists')}>
            {#snippet icon()}<Users class="w-5 h-5" />{/snippet}
          </StatCard>
          <StatCard value={stats.peer_count} label={t('stats.peers')}>
            {#snippet icon()}<Radio class="w-5 h-5" />{/snippet}
          </StatCard>
        </div>
      </section>
    {/if}

    <!-- ─── 9. Surprise Me ─────────────────────────────────────────── -->
    <section>
      <h2 class="text-xl font-bold mb-4">{t('explore.surpriseMe')}</h2>
      <button
        class="w-full relative overflow-hidden rounded-2xl bg-gradient-to-r from-[hsl(var(--primary))] via-purple-600 to-pink-500 p-6 md:p-8 text-left group transition-all hover:shadow-xl hover:shadow-[hsl(var(--primary)/0.2)] active:scale-[0.99]"
        onclick={handleSurpriseMe}
        disabled={surpriseLoading}
      >
        <!-- Background decoration -->
        <div class="absolute -top-10 -right-10 w-40 h-40 bg-white/10 rounded-full blur-2xl group-hover:scale-150 transition-transform duration-500"></div>
        <div class="absolute -bottom-8 -left-8 w-32 h-32 bg-white/5 rounded-full blur-2xl"></div>

        <div class="relative flex items-center gap-4">
          <div class="w-14 h-14 rounded-full bg-white/20 backdrop-blur-sm flex items-center justify-center flex-shrink-0 group-hover:scale-110 transition-transform">
            {#if surpriseLoading}
              <div class="w-6 h-6 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
            {:else}
              <Shuffle class="w-7 h-7 text-white" />
            {/if}
          </div>
          <div>
            <h3 class="text-xl md:text-2xl font-bold text-white">{t('explore.surpriseMe')}</h3>
            <p class="text-sm text-white/70 mt-0.5">Play 20 random tracks from your library</p>
          </div>
          <div class="ml-auto hidden md:flex items-center justify-center w-10 h-10 rounded-full bg-white/20 group-hover:bg-white/30 transition">
            <Play class="w-5 h-5 text-white ml-0.5" fill="currentColor" />
          </div>
        </div>
      </button>
    </section>

  </div>
{/if}
