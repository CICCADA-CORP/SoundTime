<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { PanelLeft, Plus, Search, Settings, UserRound } from "lucide-svelte";
  import { Button } from "$lib/components/ui/button";
  import { t, type TranslationKey } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { admin, catalog, discover, isActive, libraryHome, personal, utilities } from "./navigation";

  let { expanded, toggleSidebar, openSearch }: {
    expanded: boolean;
    toggleSidebar: () => void;
    openSearch: () => void;
  } = $props();

  const auth = getAuthStore();
  const sections = [...discover, ...catalog, libraryHome, ...personal, ...utilities, admin];
  const otherPages: Record<string, TranslationKey> = {
    "/login": "auth.login",
    "/register": "auth.register",
    "/search": "common.search",
    "/tos": "tos.title",
  };
  const path = $derived(page.url.pathname);
  const section = $derived(sections.find(item => isActive(path, item.href))?.label ?? otherPages[path]);
  const mac = typeof navigator !== "undefined" && /Mac|iPad|iPhone/i.test(navigator.platform);
</script>

<header class="workspace-header" class:local-search={path === "/explore"}>
  <div class="location">
    <button id="sidebar-toggle" class="icon-button" type="button" aria-label={t(expanded ? "workspace.hideMenu" : "workspace.showMenu")}
      aria-expanded={expanded} aria-controls={expanded ? "workspace-sidebar" : undefined} onclick={toggleSidebar}
      title={t(expanded ? "workspace.hideMenu" : "workspace.showMenu")}>
      <PanelLeft size={18} strokeWidth={1.6} />
    </button>
    <span class="section-name">{section ? t(section) : "SoundTime"}</span>
  </div>

  <div class="actions">
    {#if auth.isAuthenticated}
    <Button href="/upload" size="sm" class="h-8 gap-2 rounded-lg px-3 text-sm shadow-none" aria-label={t("nav.upload")}>
      <Plus size={15} /><span class="upload-label">{t("nav.upload")}</span>
    </Button>
    {/if}
    {#if path !== "/explore"}
    <button class="search-trigger" type="button" aria-label={t("common.search")} aria-haspopup="dialog"
      aria-keyshortcuts="Meta+K Control+K" onclick={(event) => { event.currentTarget.focus(); openSearch(); }}>
      <Search size={17} strokeWidth={1.65} />
      <span>{t("search.placeholder")}</span>
      <kbd>{mac ? "⌘K" : "Ctrl K"}</kbd>
    </button>
    {/if}
  </div>

  <div class="preferences">
    {#if auth.isAuthenticated}
      <a class="icon-button" href="/settings" aria-label={t("nav.settings")} title={t("nav.settings")}><Settings size={18} strokeWidth={1.6} /></a>
    {:else}
      <a class="icon-button" href={signInLink(page.url)} aria-label={t("nav.signIn")} title={t("nav.signIn")}><UserRound size={18} strokeWidth={1.6} /></a>
    {/if}
  </div>
</header>

<style>
  .workspace-header {
    display: grid; grid-template-columns: minmax(0, 1fr) minmax(250px, min(53%, 510px)) minmax(36px, 1fr);
    align-items: center; flex: none; gap: 12px; min-height: 48px; padding: 6px 16px;
    background: hsl(var(--background));
  }
  .workspace-header.local-search { grid-template-columns: minmax(0, 1fr) auto auto; }
  .location, .actions, .preferences { display: flex; align-items: center; min-width: 0; }
  .location { gap: 10px; }
  .section-name { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: 14px; font-weight: 520; }
  .actions { gap: 8px; }
  .preferences { justify-content: flex-end; }
  .icon-button { flex: none; display: grid; place-items: center; width: 32px; height: 32px; border-radius: 6px; color: hsl(var(--muted-foreground)); }
  .icon-button:hover { background: hsl(var(--muted)); color: hsl(var(--foreground)); }
  .search-trigger {
    display: flex; align-items: center; flex: 1; min-width: 0; height: 32px; gap: 8px; padding: 0 8px 0 11px;
    border: 1px solid hsl(var(--border)); border-radius: 9px; color: hsl(var(--muted-foreground));
    background: hsl(var(--sidebar)); text-align: left; font-size: 14px;
  }
  .search-trigger:hover { border-color: hsl(var(--input)); }
  .search-trigger :global(svg), kbd { flex: none; }
  .search-trigger span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  kbd { margin-left: auto; padding: 0 4px; border: 1px solid hsl(var(--border)); border-radius: 4px; font-family: inherit; font-size: 11px; font-weight: 500; line-height: 18px; }
  @media (max-width: 950px) {
    .workspace-header { grid-template-columns: minmax(0, 1fr) minmax(200px, 55%) minmax(32px, .25fr); gap: 4px; }
  }
  @media (max-width: 767px) {
    .workspace-header { grid-template-columns: minmax(0, 1fr) auto auto; gap: 2px; min-height: 54px; padding: 4px 12px; border-bottom: 1px solid hsl(var(--border)); }
    .location { gap: 5px; }
    .section-name { font-size: 15px; font-weight: 600; }
    .actions { gap: 4px; }
    .icon-button, .search-trigger { width: 40px; height: 40px; }
    .search-trigger { flex: none; justify-content: center; padding: 0; background: transparent; border: 0; color: hsl(var(--muted-foreground)); }
    .search-trigger span, kbd { display: none; }
  }
  @media (max-width: 360px) {
    .upload-label { display: none; }
  }
</style>
