<script lang="ts">
  import CatalogArtwork from "./CatalogArtwork.svelte";
  import type { Snippet } from "svelte";
  let { kind, title, image, label, info, actions }: {
    kind: "album" | "artist" | "playlist"; title: string; image?: string | null; label: string; info?: Snippet; actions?: Snippet;
  } = $props();
</script>

<header class="flex flex-col gap-5 sm:flex-row sm:items-end sm:gap-7">
  <div class="size-36 shrink-0 sm:size-48">
    <CatalogArtwork {kind} {title} source={image} />
  </div>
  <div class="min-w-0 space-y-2">
    <p class="text-sm font-medium text-primary">{label}</p>
    <h1 class="break-words text-3xl font-semibold leading-tight tracking-tight sm:text-4xl">{title}</h1>
    {#if info}<div class="max-w-2xl text-sm text-muted-foreground">{@render info()}</div>{/if}
    {#if actions}<div class="flex flex-wrap items-center gap-2 pt-2">{@render actions()}</div>{/if}
  </div>
</header>
