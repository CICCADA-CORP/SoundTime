<script lang="ts">
  import type { Artist } from "$lib/types";

  export let artist: Artist;
</script>

<a href="/artists/{artist.id}" class="group block">
  <div class="bg-[hsl(var(--card))] rounded-lg p-4 hover:bg-[hsl(var(--secondary))] transition">
    <div class="aspect-square rounded-full bg-[hsl(var(--secondary))] mb-3 overflow-hidden mx-auto w-36 h-36">
      {#if artist.image_url}
        <img src={artist.image_url} alt={artist.name} class="w-full h-full object-cover" />
      {:else}
        <div class="w-full h-full flex items-center justify-center text-4xl">🎤</div>
      {/if}
    </div>
    <h3 class="text-sm font-medium truncate text-center">{artist.name}</h3>
    <p class="text-xs text-[hsl(var(--muted-foreground))] text-center mt-1">Artist</p>
  </div>
</a>
