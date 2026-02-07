<script lang="ts">
  import type { Album } from "$lib/types";

  export let album: Album;
</script>

<a href="/albums/{album.id}" class="group block">
  <div class="bg-[hsl(var(--card))] rounded-lg p-4 hover:bg-[hsl(var(--secondary))] transition">
    <div class="aspect-square rounded-md bg-[hsl(var(--secondary))] mb-3 overflow-hidden relative">
      {#if album.cover_url}
        <img src={album.cover_url} alt={album.title} class="w-full h-full object-cover" />
      {:else}
        <div class="w-full h-full flex items-center justify-center text-4xl">💿</div>
      {/if}
      <button aria-label="Play album" class="absolute bottom-2 right-2 w-10 h-10 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center opacity-0 group-hover:opacity-100 transition transform translate-y-2 group-hover:translate-y-0 shadow-lg">
        <svg class="w-5 h-5 ml-0.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
      </button>
    </div>
    <h3 class="text-sm font-medium truncate">{album.title}</h3>
    <p class="text-xs text-[hsl(var(--muted-foreground))] truncate mt-1">
      {album.year ?? ""} {album.artist_name ? `· ${album.artist_name}` : ""}
    </p>
  </div>
</a>
