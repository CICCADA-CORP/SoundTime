<script lang="ts">
  import type { Snippet } from "svelte";
  import CatalogArtwork from "./CatalogArtwork.svelte";

  let { href, title, kind, artworkId, image, description, detail, actions }: {
    href: string; title: string; kind: "album" | "artist" | "playlist"; artworkId: string; image?: string | null;
    description?: string | null; detail?: string; actions?: Snippet;
  } = $props();
</script>

<article class="group">
  <a {href} aria-label={title}>
    <div class="preview" class:portrait={kind === "artist"}><CatalogArtwork {kind} {title} {artworkId} source={image} /></div>
    <div class="caption">
      <h3 title={title}>{title}</h3>
      {#if description}<p>{description}</p>{/if}
      {#if detail}<p class="detail">{detail}</p>{/if}
    </div>
  </a>
  {#if actions}{@render actions()}{/if}
</article>

<style>
  article { position: relative; min-width: 0; }
  a { display: block; border-radius: var(--art-radius); }
  .preview { width: 100%; aspect-ratio: 1; overflow: hidden; border-radius: var(--art-radius); background: hsl(var(--muted));
    box-shadow: 0 0 0 1px var(--glass-edge), 0 5px 15px -8px hsl(var(--foreground) / .22); }
  .portrait { border-radius: 50%; }
  .caption { min-width: 0; padding: .68rem .12rem .15rem; }
  h3, p { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  h3 { font-size: .925rem; line-height: 1.32; font-weight: 600; letter-spacing: -.015em; }
  p { margin-top: .18rem; color: hsl(var(--muted-foreground)); font-size: .79rem; line-height: 1.4; }
  .detail { font-variant-numeric: tabular-nums; }
  a:hover h3, a:focus-visible h3 { text-decoration: underline; text-underline-offset: .18em; }
</style>
