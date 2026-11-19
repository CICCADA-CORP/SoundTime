<script lang="ts">
  import type { Snippet } from "svelte";
  import CatalogArtwork from "./CatalogArtwork.svelte";

  let { href, title, kind, image, description, detail, actions }: {
    href: string;
    title: string;
    kind: "album" | "artist" | "playlist";
    image?: string | null;
    description?: string | null;
    detail?: string;
    actions?: Snippet;
  } = $props();
</script>

<article class="group relative min-w-0 rounded-xl border border-border bg-card p-2">
  <a {href} aria-label={title} class="block rounded-md">
    <div class="preview">
      <CatalogArtwork {kind} {title} source={image} />
    </div>
    <div class="px-1 pb-1.5 pt-2.5">
      <h3 class="truncate text-sm font-medium leading-5 group-hover:underline">{title}</h3>
      {#if description}<p class="mt-0.5 truncate text-xs leading-5 text-muted-foreground">{description}</p>{/if}
      {#if detail}<p class="truncate text-xs leading-5 text-muted-foreground">{detail}</p>{/if}
    </div>
  </a>
  {#if actions}{@render actions()}{/if}
</article>

<style>
  .preview { aspect-ratio: 8 / 5; overflow: hidden; border-radius: 7px; background: hsl(var(--muted)); }
  .preview :global(.artwork) { border-radius: 0; }
</style>
