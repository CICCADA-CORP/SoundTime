<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import type { Snippet } from "svelte";

  let { title, category, href, linkLabel, actions }: {
    title: string; category?: string; href?: string; linkLabel?: string; actions?: Snippet;
  } = $props();
</script>

<header class="section-heading">
  <div>
    {#if category}<p>{category}</p>{/if}
    <h2>{title}</h2>
  </div>
  {#if href}<a {href} aria-label={linkLabel ?? `${t("explore.viewAll")} · ${title}`}>{linkLabel ?? t("explore.viewAll")}</a>{/if}
  {#if actions}{@render actions()}{/if}
</header>

<style>
  .section-heading { display: flex; align-items: flex-end; justify-content: space-between; gap: .75rem; margin-bottom: .95rem; }
  .section-heading > div { min-width: 0; }
  h2 { font-size: clamp(1.2rem, 1.65cqi + .65rem, 1.55rem); font-weight: 630; letter-spacing: -.035em; line-height: 1.15; overflow-wrap: anywhere; }
  p, a { font-size: .82rem; line-height: 1.45; }
  p { margin-bottom: .22rem; color: hsl(var(--muted-foreground)); }
  a { flex: none; padding: .25rem 0; color: hsl(var(--link)); font-weight: 550; }
  a:hover { text-decoration: underline; text-underline-offset: 3px; }
</style>
