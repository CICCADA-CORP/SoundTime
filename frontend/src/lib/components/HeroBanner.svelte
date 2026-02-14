<script lang="ts">
  import { Play } from "lucide-svelte";
  import type { EditorialPlaylist, Album } from "$lib/types";

  let { item, type = "editorial", onclick }: {
    item: EditorialPlaylist | Album;
    type?: "editorial" | "album";
    onclick?: () => void;
  } = $props();

  const title = type === "editorial" ? (item as EditorialPlaylist).name : (item as Album).title;
  const description = type === "editorial"
    ? (item as EditorialPlaylist).description
    : (item as Album).artist_name ?? null;
  const coverUrl = item.cover_url;
  const trackCount = type === "editorial" ? (item as EditorialPlaylist).track_count : undefined;
</script>

<button
  class="relative w-full h-56 md:h-72 rounded-2xl overflow-hidden group cursor-pointer text-left"
  {onclick}
>
  {#if coverUrl}
    <img
      src={coverUrl}
      alt={title}
      class="absolute inset-0 w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
    />
  {:else}
    <div class="absolute inset-0 bg-gradient-to-br from-[hsl(var(--primary))] to-purple-700"></div>
  {/if}
  <div class="absolute inset-0 bg-gradient-to-t from-black/80 via-black/30 to-transparent"></div>
  <div class="absolute bottom-0 left-0 right-0 p-6 md:p-8">
    <p class="text-xs uppercase tracking-wider text-white/70 mb-1">
      {type === "editorial" ? "Featured Playlist" : "Featured Album"}
    </p>
    <h2 class="text-2xl md:text-3xl font-bold text-white mb-1">{title}</h2>
    {#if description}
      <p class="text-sm text-white/80 line-clamp-2 max-w-lg">{description}</p>
    {/if}
    <div class="flex items-center gap-3 mt-3">
      <div class="w-10 h-10 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center shadow-lg group-hover:scale-110 transition-transform">
        <Play class="w-5 h-5 ml-0.5" fill="currentColor" />
      </div>
      {#if trackCount}
        <span class="text-sm text-white/70">{trackCount} tracks</span>
      {/if}
    </div>
  </div>
</button>
