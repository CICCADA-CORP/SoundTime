<script lang="ts">
  import CatalogArtwork from "./CatalogArtwork.svelte";
  import ReleaseFrame from "./ReleaseFrame.svelte";
  import type { Snippet } from "svelte";
  let { kind, artworkId, title, image, label, info, actions }: {
    kind: "album" | "artist" | "playlist"; artworkId: string; title: string; image?: string | null; label: string; info?: Snippet; actions?: Snippet;
  } = $props();
</script>

<ReleaseFrame portrait={kind === "artist"}>
  {#snippet cover()}<CatalogArtwork {kind} {title} {artworkId} source={image} loading="eager" />{/snippet}
  <p class="type">{label}</p>
  <h1>{title}</h1>
  {#if info}<div class="credits">{@render info()}</div>{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</ReleaseFrame>

<style>
  .type { color: hsl(var(--muted-foreground)); font-size: .79rem; line-height: 1.4; font-weight: 550; }
  h1 { margin-top: .32rem; font-size: clamp(1.9rem, 6.3cqi, 4.3rem); line-height: 1.015; font-weight: 650; letter-spacing: -.052em; overflow-wrap: anywhere; }
  .credits { margin-top: .55rem; font-size: .88rem; line-height: 1.5; color: hsl(var(--muted-foreground)); }
  .credits :global(a) { color: hsl(var(--link)); text-underline-offset: .16em; }
  .actions { margin-top: 1.15rem; display: flex; align-items: center; flex-wrap: wrap; gap: .48rem; }
  .actions :global([data-slot="button"]) { border-radius: 999px; }
  .actions :global(.play-release) { min-height: 2.45rem; padding-inline: 1.2rem; }
</style>
