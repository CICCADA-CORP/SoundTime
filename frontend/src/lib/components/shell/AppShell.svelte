<script lang="ts">
  import { afterNavigate } from "$app/navigation";
  import { tick, untrack, type Snippet } from "svelte";
  import { MediaQuery } from "svelte/reactivity";
  import { Dialog } from "bits-ui";
  import { live } from "$lib/webradios/live.svelte";
  import { podcast } from "$lib/webradios/podcast.svelte";
  import { webPreferences } from "$lib/webradios/preferences.svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { canPlay } from "$lib/catalog/availability";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import AudioPlayer from "../AudioPlayer.svelte";
  import QueuePanel from "../player/QueuePanel.svelte";
  import SearchDialog from "./SearchDialog.svelte";
  import SideNavigation from "./SideNavigation.svelte";
  import TaskBanner from "./TaskBanner.svelte";
  import WorkspaceHeader from "./WorkspaceHeader.svelte";

  let { children }: { children: Snippet } = $props();
  const auth = getAuthStore();
  const player = getPlayerStore();
  const queue = getQueueStore();
  const desktop = new MediaQuery("(min-width: 768px)");
  const wide = new MediaQuery("(min-width: 1160px)");
  let main: HTMLElement;
  let menuOpen = $state(false);
  let searchOpen = $state(false);
  let panelOpen = $state(true);
  let sheetOpen = $state(false);
  let usedQueue = $state(false);
  let queueTrigger: HTMLElement | null = null;
  const visible = $derived(wide.current ? panelOpen : sheetOpen);
  const panelTitle = $derived(t(player.currentTrack && !live.station && !podcast.episode ? "player.nowPlaying" : "player.queue"));
  const suggestions = catalogQuery(signal => (wide.current || usedQueue) && !player.currentTrack && !queue.queue.length ? homeApi.recentlyAdded(24, signal) : null);
  const recent = $derived.by(() => {
    const playable = suggestions.data?.data?.filter(canPlay) ?? [];
    const illustrated = playable.find(track => track.cover_url?.trim());
    if (!illustrated) return playable.slice(0, 8);
    return [illustrated, ...playable.filter(track => track.id !== illustrated.id).slice(0, 7)];
  });

  $effect(() => {
    const id = auth.user?.id ?? null;
    untrack(() => { void webPreferences.refresh(id); });
  });

  $effect(() => {
    if (!live.station && !podcast.episode) return;
    const id = auth.user?.id ?? null;
    const refresh = () => { void webPreferences.refresh(id); };
    const interval = window.setInterval(refresh, 30_000);
    window.addEventListener("focus", refresh);
    return () => { window.clearInterval(interval); window.removeEventListener("focus", refresh); };
  });

  async function toggleQueue(trigger: HTMLElement) {
    queueTrigger = trigger;
    usedQueue = true;
    if (!wide.current) { sheetOpen = true; return; }
    panelOpen = !panelOpen;
    if (panelOpen) {
      await tick();
      document.querySelector<HTMLElement>("#workspace-queue [data-close-queue]")?.focus();
    }
  }

  async function closeQueue() {
    if (!wide.current) { sheetOpen = false; return; }
    panelOpen = false;
    await tick();
    document.querySelector<HTMLElement>("[data-dock-queue]")?.focus();
  }

  function queueKeys(event: KeyboardEvent) {
    if (event.key === "Escape" && !event.defaultPrevented && event.target instanceof Element && event.target.closest("#workspace-queue") && wide.current) {
      event.preventDefault();
      void closeQueue();
    }
  }

  afterNavigate(({ from, type }) => {
    menuOpen = false;
    sheetOpen = false;
    if (from && type !== "popstate") main?.scrollTo(0, 0);
  });
</script>

<svelte:window onkeydown={queueKeys} />

<div class="workspace">
  <a class="skip-link" href="#main-content">{t("shell.skipToContent")}</a>
  <div class="workspace-window" class:docked={wide.current && panelOpen}>
    {#if desktop.current}
      <aside class="library-sidebar" id="workspace-directory" aria-label={t("shell.navigation")}>
        <SideNavigation rail />
      </aside>
    {/if}
    <div class="center-panel">
      <WorkspaceHeader mobile={!desktop.current} {menuOpen} openMenu={() => menuOpen = true} openSearch={() => searchOpen = true} queueVisible={visible} openQueue={toggleQueue} />
      {#if auth.isAdmin}<TaskBanner />{/if}
      <main id="main-content" tabindex="-1" bind:this={main}>
        <div class="page-content">{@render children()}</div>
      </main>
    </div>
    {#if wide.current && panelOpen}
      <aside class="listening-sidebar" id="workspace-queue" aria-label={panelTitle}>
        <QueuePanel {recent} title={panelTitle} loading={suggestions.loading} close={closeQueue} />
      </aside>
    {/if}
  </div>
  <AudioPlayer queueVisible={visible} toggleQueue={toggleQueue} queueAsDialog={!wide.current} />
</div>

<Dialog.Root open={menuOpen && !desktop.current} onOpenChange={open => menuOpen = open}>
  <Dialog.Portal>
    <Dialog.Overlay class="workspace-scrim" />
    <Dialog.Content id="workspace-directory" class="workspace-sheet" onCloseAutoFocus={event => {
      event.preventDefault();
      document.getElementById("directory-toggle")?.focus();
    }}>
      <Dialog.Title class="sr-only">{t("shell.navigation")}</Dialog.Title>
      <SideNavigation close={() => menuOpen = false} />
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root open={sheetOpen && !wide.current} onOpenChange={open => sheetOpen = open}>
  <Dialog.Portal>
    <Dialog.Overlay class="workspace-scrim" />
    <Dialog.Content id="workspace-queue" class="workspace-sheet" onCloseAutoFocus={event => {
      event.preventDefault();
      queueTrigger?.focus();
    }}>
      <Dialog.Title class="sr-only">{t("player.queue")}</Dialog.Title>
      <QueuePanel {recent} title={panelTitle} loading={suggestions.loading} close={closeQueue} />
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<SearchDialog bind:open={searchOpen} />

<style>
  .workspace { width: 100%; height: 100dvh; min-width: 0; display: flex; flex-direction: column; color: hsl(var(--foreground)); background: hsl(var(--background)); }
  .workspace > :global(.live-bar), .workspace > :global(.episode-bar) { flex: none; position: relative; inset: auto; width: 100%; }
  .workspace-window { --sidebar-width: clamp(176px,14vw,196px); --queue-width: 0px; flex: 1; min-width: 0; min-height: 0; display: grid; grid-template: minmax(0,1fr) / var(--sidebar-width) minmax(0,1fr) var(--queue-width); }
  .workspace-window.docked { --queue-width: clamp(240px,18.4vw,262px); }
  .library-sidebar, .listening-sidebar { min-width: 0; min-height: 0; }
  .library-sidebar { grid-area: 1 / 1; background: hsl(var(--sidebar)); border-inline-end: 1px solid hsl(var(--border) / .65); }
  .center-panel { grid-area: 1 / 2; min-width: 0; min-height: 0; display: flex; flex-direction: column; container-type: inline-size; background: hsl(var(--background)); }
  .listening-sidebar { grid-area: 1 / 3; padding: 13px 14px 15px 1px; background: hsl(var(--background)); }
  main { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; scroll-padding: 15px; }
  .page-content { width: 100%; max-width: 1460px; margin-inline: auto; padding: 0 clamp(18px,3.4cqi,38px) 38px; }
  :global(.workspace-scrim) { position: fixed; z-index: 70; inset: 0; background: rgb(14 18 33 / .38); }
  :global(.workspace-sheet) { position: fixed; z-index: 71; inset: 0; margin: auto; width: min(410px, calc(100vw - 36px)); height: min(720px, calc(100dvh - 68px)); overflow: hidden; background: hsl(var(--popover)); color: hsl(var(--popover-foreground)); border: 1px solid var(--glass-edge); border-radius: 18px; box-shadow: var(--glass-shadow); }
  @media (min-width: 768px) and (max-width: 1000px) { .workspace-window { --sidebar-width: 69px; } }
  @media (max-width: 767px) {
    .workspace-window { --sidebar-width: 0px; }
    .page-content { padding: 3px clamp(12px,3.6vw,23px) 30px; }
    :global(.workspace-sheet) { inset: auto 0 0; width: 100%; height: min(86dvh, 760px); padding-bottom: env(safe-area-inset-bottom); border-radius: 20px 20px 0 0; }
  }
</style>
