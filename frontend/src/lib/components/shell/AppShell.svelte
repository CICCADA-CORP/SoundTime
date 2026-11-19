<script lang="ts">
  import { afterNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import type { Snippet } from "svelte";
  import { MediaQuery } from "svelte/reactivity";
  import { Dialog } from "bits-ui";
  import { LibraryBig } from "lucide-svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import AudioPlayer from "../AudioPlayer.svelte";
  import SearchDialog from "./SearchDialog.svelte";
  import SideNavigation from "./SideNavigation.svelte";
  import TaskBanner from "./TaskBanner.svelte";
  import WorkspaceHeader from "./WorkspaceHeader.svelte";
  import { catalog, discover, isActive, libraryHome, personal } from "./navigation";

  let { children }: { children: Snippet } = $props();
  const auth = getAuthStore();
  const player = getPlayerStore();
  const desktop = new MediaQuery("(min-width: 768px)");
  const libraryPages = [libraryHome, ...catalog, ...personal];
  let main: HTMLElement;
  let collapsed = $state(false);
  let mobileMenu = $state(false);
  let searchOpen = $state(false);
  const expanded = $derived(desktop.current ? !collapsed : mobileMenu);
  const path = $derived(page.url.pathname);

  function toggleSidebar() {
    if (desktop.current) collapsed = !collapsed;
    else mobileMenu = !mobileMenu;
  }

  afterNavigate(({ from, type }) => {
    mobileMenu = false;
    if (from && type !== "popstate") main?.scrollTo(0, 0);
  });
</script>

<div class={["workspace", { collapsed }]}>
  <a class="skip-link" href="#main-content">{t("shell.skipToContent")}</a>
  {#if desktop.current}
    <div id="workspace-sidebar" class="sidebar-column" hidden={collapsed}><SideNavigation /></div>
  {/if}

  <div class="frame">
    <WorkspaceHeader {expanded} {toggleSidebar} openSearch={() => searchOpen = true} />
    {#if auth.isAdmin}<TaskBanner />{/if}
    <main id="main-content" tabindex="-1" bind:this={main} class={{ playing: !!player.currentTrack }}>
      <div class="page-content">{@render children()}</div>
    </main>
  </div>

  {#if !desktop.current}
    <nav class="mobile-tabs" aria-label={t("shell.navigation")}>
      {#each discover as item (item.href)}
        <a href={item.href} aria-current={isActive(path, item.href) ? "page" : undefined}>
          <item.icon size={20} strokeWidth={1.65} /><span>{t(item.label)}</span>
        </a>
      {/each}
      <a href="/library" aria-current={libraryPages.some(item => isActive(path, item.href)) ? "location" : undefined}>
        <LibraryBig size={20} strokeWidth={1.65} /><span>{t("shell.collection")}</span>
      </a>
    </nav>
  {/if}

  <AudioPlayer />
</div>

<Dialog.Root open={mobileMenu && !desktop.current} onOpenChange={(open) => mobileMenu = open}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-[70] bg-black/25" />
    <Dialog.Content id="workspace-sidebar" class="sidebar-sheet" onCloseAutoFocus={(event) => {
      event.preventDefault();
      document.getElementById("sidebar-toggle")?.focus();
    }}>
      <Dialog.Title class="sr-only">{t("shell.navigation")}</Dialog.Title>
      <SideNavigation mobile close={() => mobileMenu = false} />
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<SearchDialog bind:open={searchOpen} />

<style>
  .workspace { --workspace-left: var(--rail-width); display: grid; grid-template-columns: var(--workspace-left) minmax(0, 1fr); height: 100dvh; }
  .workspace.collapsed { --workspace-left: 0px; }
  .sidebar-column { grid-column: 1; grid-row: 1; min-width: 0; min-height: 0; }
  .frame { grid-column: 2; grid-row: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  main { flex: 1; min-height: 0; overflow-y: auto; overscroll-behavior-y: contain; }
  .page-content { max-width: 1680px; margin-inline: auto; padding: 34px clamp(24px, 2.8vw, 48px) 48px; }
  main.playing { padding-bottom: 76px; scroll-padding-bottom: 76px; }
  :global(.sidebar-sheet) { position: fixed; z-index: 71; inset: 0 auto 0 0; width: min(290px, 88vw); background: hsl(var(--sidebar)); }
  .mobile-tabs { display: none; }
  @media (max-width: 767px) {
    .workspace, .workspace.collapsed { --workspace-left: 0px; }
    .page-content { padding: 24px 16px 32px; }
    main { padding-bottom: var(--mobile-nav); }
    main.playing { padding-bottom: calc(var(--mobile-nav) + 62px); scroll-padding-bottom: calc(var(--mobile-nav) + 62px); }
    .mobile-tabs {
      position: fixed; z-index: 48; inset: auto 0 0; height: var(--mobile-nav);
      display: flex; align-items: stretch; padding-bottom: env(safe-area-inset-bottom);
      border-top: 1px solid hsl(var(--border)); background: hsl(var(--background));
    }
    .mobile-tabs a { display: flex; flex: 1; align-items: center; justify-content: center; flex-direction: column; gap: 3px; color: hsl(var(--muted-foreground)); font-size: 11px; }
    .mobile-tabs a[aria-current] { color: hsl(var(--foreground)); font-weight: 650; }
    .mobile-tabs a[aria-current] :global(svg) { stroke-width: 2.15; }
    .mobile-tabs :focus-visible { outline-offset: -3px; }
  }
</style>
