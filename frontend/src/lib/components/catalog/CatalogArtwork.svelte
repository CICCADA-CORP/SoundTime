<script lang="ts">
  import { Disc3, ListMusic, UserRound } from "lucide-svelte";

  let { source, title, kind }: { source?: string | null; title: string; kind: "album" | "artist" | "playlist" } = $props();
  let broken = $state<string | null>(null);
  const icons = { album: Disc3, artist: UserRound, playlist: ListMusic };
  let Icon = $derived(icons[kind]);
</script>

<span class="artwork {kind}">
  {#if source && broken !== source}
    <img src={source} alt={title} loading="lazy" class="size-full object-cover" onerror={() => broken = source ?? null} />
  {:else}
    <span role="img" aria-label={title} class="fallback"><Icon size={42} strokeWidth={1.25} /></span>
  {/if}
</span>

<style>
  .artwork { display: grid; height: 100%; width: 100%; overflow: hidden; border-radius: .6rem; background: hsl(var(--secondary)); }
  .artist { border-radius: 50%; }
  .fallback { display: grid; place-items: center; background: hsl(var(--muted)); color: hsl(var(--muted-foreground)); }
</style>
