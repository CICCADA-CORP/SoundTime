<script lang="ts">
  import { webPreferences } from "$lib/webradios/preferences.svelte";
  import { recentPlaylists } from "$lib/catalog/playlists.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import type { Playlist } from "$lib/types";
  import { page } from "$app/state";
  import CatalogArtwork from "../catalog/CatalogArtwork.svelte";
  import { admin, catalog, followSidebarLink, home, isActive, libraryHome, listening, personal, settings, upload, type NavItem } from "./navigation";

  let { close, rail = false }: { close?: () => void; rail?: boolean } = $props();
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
  <a href={item.href} class="row" aria-label={t(item.label)} title={t(item.label)} onclick={(event) => followSidebarLink(event, close)}
    aria-current={isActive(page.url.pathname, item.href) && !(item.href === "/playlists" && recentIsCurrent) ? "page" : undefined}>
    <span class="tile" aria-hidden="true"><item.icon size={16} strokeWidth={1.7} /></span>
    <span>{t(item.label)}</span>
  </a>
{/snippet}

<nav class:rail aria-label={t("shell.navigation")}>

  <div class="group">
    <h2>{t("workspaceNav.listen")}</h2>
    {@render destination(home)}
    {#each listening.filter(item => item.href !== "/webradios" || webPreferences.showInNavigation) as item (item.href)}{@render destination(item)}{/each}
    {#if !auth.isAuthenticated && webPreferences.status?.instance_enabled !== false && !webPreferences.userEnabled}
      {@render destination({ ...settings, href: "/settings#webradios", label: "webradios.preferenceLink" })}
    {/if}
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
        <a {href} class="row" aria-label={playlist.name} title={playlist.name} aria-current={isActive(page.url.pathname, href) ? "page" : undefined}
          onclick={(event) => followSidebarLink(event, close)}>
          <span class="cover" aria-hidden="true"><CatalogArtwork source={playlist.cover_url} title={playlist.name} kind="playlist" artworkId={playlist.id} /></span>
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
  nav { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-content: start; gap: 22px; padding: 22px 2px; }
  h2 { color: hsl(var(--muted-foreground)); font-size: .76rem; line-height: 1.4; font-weight: 580; margin: 0 7px 6px; }
  .row { display: flex; align-items: center; gap: 10px; min-height: 36px; padding: 4px 7px; border-radius: 7px; color: hsl(var(--foreground)); font-size: .91rem; line-height: 1.3; }
  .tile, .cover { display: grid; place-items: center; flex: none; width: 22px; height: 22px; }
  .tile { color: hsl(var(--muted-foreground)); }
  .cover { overflow: hidden; }
  .row:hover, .row[aria-current] { background: hsl(var(--muted)); }
  .row[aria-current] .tile { color: hsl(var(--primary)); }
  .row[aria-current] { font-weight: 650; }
  .row:focus-visible { outline-offset: -2px; }
  .playlist-name { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .cover :global(.artwork) { border-radius: 4px; }
  .cover :global(svg) { width: 14px; height: 14px; }
  @media (max-width: 680px) {
    nav { gap: 20px 12px; }
    .row { min-height: 44px; }
  }
  .rail { grid-template-columns: minmax(0,1fr); gap: 18px; padding: 4px 1px 14px; }
  .rail h2 { font-size: .67rem; font-weight: 550; margin: 0 9px 5px; }
  .rail .row { min-height: 32px; gap: 8px; font-size: .8rem; line-height: 1.3; }
  .rail .row:hover { background: hsl(var(--card) / .42); }
  .rail .row[aria-current] { background: hsl(var(--card) / .72); box-shadow: 0 1px 5px rgb(24 28 56 / .05); }
  .rail .tile { color: hsl(var(--primary)); }
  .rail .tile, .rail .cover { width: 20px; height: 20px; }
  @media (min-width: 768px) and (max-width: 1000px) {
    .rail .group + .group { border-top: 1px solid var(--glass-edge); padding-top: 8px; }
    .rail { gap: 9px; }
    .rail h2, .rail .row > span:last-child { display: none; }
    .rail .row { justify-content: center; min-height: 36px; }
  }
</style>
