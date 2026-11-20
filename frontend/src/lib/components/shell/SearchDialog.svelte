<script lang="ts">
  import { Dialog } from "bits-ui";
  import { ArrowRight, Disc3, Music2, Search, UserRound, X } from "lucide-svelte";
  import { goto } from "$app/navigation";
  import { apiFetch } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import type { SearchResults } from "$lib/types";

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let query = $state("");
  let input: HTMLInputElement;
  let suggestions = $state.raw<SearchResults | null>(null);
  let failed = $state(false);
  let navigating = false;
  const term = $derived(query.trim());
  const allResults = $derived("/search?q=" + encodeURIComponent(term));

  const rows = $derived(suggestions ? [
    ...suggestions.tracks.map(track => ({ url: "/tracks/" + encodeURIComponent(track.id), label: track.title, detail: track.artist_name, kind: "track.trackLabel" as const, icon: Music2 })),
    ...suggestions.albums.map(album => ({ url: "/albums/" + encodeURIComponent(album.id), label: album.title, detail: album.artist_name, kind: "track.album" as const, icon: Disc3 })),
    ...suggestions.artists.map(artist => ({ url: "/artists/" + encodeURIComponent(artist.id), label: artist.name, detail: null, kind: "track.artist" as const, icon: UserRound })),
  ] : []);

  $effect(() => {
    const current = term;
    suggestions = null;
    failed = false;
    if (!open || !current) return;
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const result = await apiFetch<SearchResults>("/search?q=" + encodeURIComponent(current) + "&per_page=2", { signal: controller.signal });
        if (!controller.signal.aborted) suggestions = result;
      } catch {
        if (!controller.signal.aborted) failed = true;
      }
    }, 200);
    return () => { clearTimeout(timer); controller.abort(); };
  });

  function shortcut(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing || event.altKey || event.shiftKey || !(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k") return;
    event.preventDefault();
    if (open) { input?.focus(); input?.select(); }
    else open = true;
  }

  function navigate(url: string) {
    navigating = true;
    open = false;
    void goto(url);
  }

  function follow(event: MouseEvent, url: string) {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    navigate(url);
  }

  function moveFocus(event: KeyboardEvent) {
    if (event.isComposing || !(event.key === "ArrowDown" || event.key === "ArrowUp") || !(event.currentTarget instanceof HTMLElement)) return;
    const items = [input, ...event.currentTarget.querySelectorAll<HTMLAnchorElement>("a[href]")];
    const current = items.indexOf(document.activeElement as HTMLInputElement);
    if (current < 0 || items.length < 2) return;
    event.preventDefault();
    const next = (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[next].focus();
  }
</script>

<svelte:window onkeydown={shortcut} />

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-[210] bg-black/25" />
    <Dialog.Content class="search-dialog" onkeydown={moveFocus}
      onOpenAutoFocus={(event) => { navigating = false; event.preventDefault(); input.focus(); }}
      onCloseAutoFocus={(event) => { query = ""; if (navigating) event.preventDefault(); }}>
      <Dialog.Title class="sr-only">{t("common.search")}</Dialog.Title>
      <form class="query-row" role="search" onsubmit={(event) => { event.preventDefault(); if (term) navigate(allResults); }}>
        <Search size={20} strokeWidth={1.7} />
        <input bind:this={input} bind:value={query} type="search" name="q" autocomplete="off"
          aria-label={t("common.search")} placeholder={t("search.placeholder")} />
        <Dialog.Close type="button" class="dismiss" aria-label={t("a11y.close")}><X size={17} /></Dialog.Close>
      </form>
      {#if term}
        <div class="results">
          {#if failed}
            <p class="message" role="status">{t("navSearch.unavailable")}</p>
          {:else if !suggestions}
            <p class="message" role="status">{t("common.loading")}</p>
          {:else if !rows.length}
            <p class="message" role="status">{t("common.noResults")}</p>
          {:else}
            <ul>
              {#each rows as row (row.url)}
                <li>
                  <a class="result" href={row.url} onclick={(event) => follow(event, row.url)}>
                    <row.icon size={17} strokeWidth={1.6} />
                    <span class="min-w-0">
                      <span class="block truncate font-medium">{row.label}</span>
                      <span class="block truncate text-xs text-muted-foreground">{t(row.kind)}{#if row.detail} · {row.detail}{/if}</span>
                    </span>
                  </a>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
        <a class="show-all" href={allResults} onclick={(event) => follow(event, allResults)}>{t("navSearch.allResults")}<ArrowRight size={16} /></a>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.search-dialog) {
    position: fixed; z-index: 211; top: max(10vh, env(safe-area-inset-top)); left: 50%; transform: translateX(-50%);
    width: min(32rem, calc(100vw - 2rem)); max-height: 75dvh; overflow-y: auto; overscroll-behavior: contain;
    border: 1px solid hsl(var(--border)); border-radius: 14px; background: hsl(var(--popover));
    color: hsl(var(--popover-foreground)); box-shadow: 0 18px 60px -16px rgb(0 0 0 / .23);
  }
  .query-row { display: flex; align-items: center; gap: 12px; min-height: 62px; padding: 0 12px 0 18px; color: hsl(var(--muted-foreground)); }
  input { width: 100%; min-width: 0; flex: 1; background: transparent; color: hsl(var(--foreground)); font-size: 16px; outline: none; }
  input::-webkit-search-cancel-button { display: none; }
  :global(.search-dialog .dismiss) { display: grid; width: 36px; height: 36px; flex-shrink: 0; place-items: center; border-radius: 6px; }
  .results { border-top: 1px solid hsl(var(--border)); padding: 5px 7px; }
  .message { padding: 13px 11px; color: hsl(var(--muted-foreground)); font-size: .875rem; }
  .result, .show-all { display: flex; align-items: center; gap: 12px; font-size: .875rem; }
  .result { min-height: 46px; padding: 5px 10px; border-radius: 6px; }
  .result :global(svg) { flex-shrink: 0; color: hsl(var(--muted-foreground)); }
  .result:focus-visible { outline-offset: -2px; }
  .result:hover, .show-all:hover, :global(.search-dialog .dismiss:hover) { background: hsl(var(--muted)); }
  .show-all { justify-content: space-between; min-height: 46px; padding: 0 18px; border-top: 1px solid hsl(var(--border)); font-weight: 550; }
</style>
