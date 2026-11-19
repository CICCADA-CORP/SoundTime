<script lang="ts">
  import { recentPlaylists } from "$lib/catalog/playlists.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import type { Playlist } from "$lib/types";
  import { page } from "$app/state";
  import CatalogArtwork from "../catalog/CatalogArtwork.svelte";
  import { admin, catalog, followSidebarLink, home, isActive, libraryHome, listening, personal, upload, type NavItem } from "./navigation";

  let { close }: { close?: () => void } = $props();
  const auth = getAuthStore();
  const playlists = recentPlaylists();
  type PublicPlaylist = Pick<Playlist, "id" | "name" | "is_public"> & Partial<Pick<Playlist, "cover_url">>;

  function validPlaylist(item: unknown): item is PublicPlaylist {
    return typeof item === "object" && item !== null &&
      "id" in item && typeof item.id === "string" && item.id.trim().length > 0 &&
      "name" in item && typeof item.name === "string" && item.name.trim().length > 0 &&
      "is_public" in item && item.is_public === true &&
      (!("cover_url" in item) || item.cover_url === null || typeof item.cover_url === "string");
  }

  const recent = $derived.by(() => {
    const data = playlists.data?.data;
    if (!Array.isArray(data)) return [];
    const unique: PublicPlaylist[] = [];
    for (const item of data) {
      if (validPlaylist(item) && !unique.some(playlist => playlist.id === item.id)) unique.push(item);
      if (unique.length === 4) break;
    }
    return unique;
  });
  const recentIsCurrent = $derived(recent.some(playlist => isActive(page.url.pathname, "/playlists/" + encodeURIComponent(playlist.id))));

  const id = $props.id();
</script>

{#snippet destination(item: NavItem)}
  <a href={item.href} class="row" onclick={(event) => followSidebarLink(event, close)}
    aria-current={isActive(page.url.pathname, item.href) && !(item.href === "/playlists" && recentIsCurrent) ? "page" : undefined}>
    <item.icon aria-hidden="true" size={17} strokeWidth={1.65} />
    <span>{t(item.label)}</span>
  </a>
{/snippet}

<nav aria-label={t("shell.navigation")}>
  {@render destination(home)}

  <div class="group">
    <h2>{t("workspaceNav.listen")}</h2>
    {#each listening as item (item.href)}{@render destination(item)}{/each}
  </div>

  <div class="group">
    <h2>{t("workspaceNav.catalog")}</h2>
    {#each catalog as item (item.href)}{@render destination(item)}{/each}
  </div>

  <div class="group">
    <h2>{t("workspaceNav.yourMusic")}</h2>
    {@render destination(libraryHome)}
    {#if auth.isAuthenticated}
      {#each personal as item (item.href)}{@render destination(item)}{/each}
      {@render destination(upload)}
    {/if}
  </div>

  {#if recent.length}
    <section class="group" aria-labelledby={id + "-recent"}>
      <h2 id={id + "-recent"}>{t("navDrawer.recentPlaylists")}</h2>
      {#each recent as playlist (playlist.id)}
        {@const href = "/playlists/" + encodeURIComponent(playlist.id)}
        <a {href} class="row" aria-current={isActive(page.url.pathname, href) ? "page" : undefined}
          onclick={(event) => followSidebarLink(event, close)}>
          <span class="cover" aria-hidden="true"><CatalogArtwork source={playlist.cover_url} title={playlist.name} kind="playlist" /></span>
          <span class="playlist-name">{playlist.name}</span>
        </a>
      {/each}
    </section>
  {/if}

  {#if auth.isAdmin}
    <div class="group">{@render destination(admin)}</div>
  {/if}
</nav>

<style>
  nav { padding: 8px 8px 18px; }
  .group { margin-top: 20px; }
  h2 {
    padding: 0 8px 5px; color: hsl(var(--muted-foreground)); font-size: 12px;
    line-height: 17px; font-weight: 450;
  }
  .row {
    display: flex; align-items: center; gap: 9px; min-height: 33px; min-width: 0;
    padding: 5px 8px; border-radius: 8px; color: color-mix(in srgb, hsl(var(--muted-foreground)) 65%, hsl(var(--foreground)));
    font-size: 14px; font-weight: 480; line-height: 20px;
  }
  .row + .row { margin-top: 2px; }
  .row :global(svg) { flex: none; color: hsl(var(--muted-foreground)); }
  .row:hover { background: hsl(var(--accent) / .55); }
  .row[aria-current] { color: hsl(var(--accent-foreground)); background: hsl(var(--accent)); font-weight: 550; }
  .row[aria-current] :global(svg) { color: currentColor; }
  .row:focus-visible { outline-offset: -2px; }
  .playlist-name { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .cover { width: 18px; height: 18px; flex: none; }
  .cover :global(.artwork) { border-radius: 3px; }
  .cover :global(.fallback) { background: hsl(var(--accent)); }
  .cover :global(svg) { width: 14px; height: 14px; }
  @media (max-width: 767px) {
    .row { min-height: 44px; }
    .group { margin-top: 16px; }
  }
</style>
