<script lang="ts">
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Artist } from "$lib/types";
  import AlbumCard from "$lib/components/AlbumCard.svelte";
  import TrackList from "$lib/components/TrackList.svelte";

  let artist: Artist | null = null;
  let loading = true;

  onMount(async () => {
    try {
      artist = await api.get<Artist>(`/artists/${$page.params.id}`);
    } catch { /* empty */ } finally { loading = false; }
  });
</script>

<svelte:head><title>{artist?.name ?? "Artist"} — SoundTime</title></svelte:head>

{#if loading}
  <div class="flex justify-center py-20">
    <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
  </div>
{:else if artist}
  <div class="space-y-8">
    <div class="flex items-end gap-6">
      <div class="w-48 h-48 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center text-6xl shadow-xl flex-shrink-0 overflow-hidden">
        {#if artist.image_url}
          <img src={artist.image_url} alt={artist.name} class="w-full h-full object-cover" />
        {:else}
          🎤
        {/if}
      </div>
      <div>
        <p class="text-xs uppercase tracking-wider text-[hsl(var(--muted-foreground))]">Artist</p>
        <h1 class="text-5xl font-bold mt-1">{artist.name}</h1>
        {#if artist.bio}
          <p class="text-sm text-[hsl(var(--muted-foreground))] mt-3 max-w-xl">{artist.bio}</p>
        {/if}
      </div>
    </div>

    {#if artist.albums && artist.albums.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-4">Albums</h2>
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-4">
        {#each artist.albums as album}
          <AlbumCard {album} />
        {/each}
      </div>
    </section>
    {/if}

    {#if artist.tracks && artist.tracks.length > 0}
    <section>
      <h2 class="text-xl font-semibold mb-4">Top Tracks</h2>
      <TrackList tracks={artist.tracks} showArtist={false} />
    </section>
    {/if}
  </div>
{:else}
  <p class="text-center py-20 text-[hsl(var(--muted-foreground))]">Artist not found.</p>
{/if}
