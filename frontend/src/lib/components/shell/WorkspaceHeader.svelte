<script lang="ts">
  import { page } from "$app/state";
  import { Popover } from "bits-ui";
  import { ChevronDown, ListMusic, Menu, Search, Settings, SunMoon, UserRound } from "lucide-svelte";
  import SoundTimeSignature from "../SoundTimeSignature.svelte";
  import { signInLink } from "$lib/access/navigation";
  import AppearanceSwitcher from "$lib/components/ui/appearance-switcher.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";

  let { mobile, menuOpen, openMenu, openSearch, queueVisible, openQueue }: {
    mobile: boolean; menuOpen: boolean; openMenu: () => void; openSearch: () => void;
    queueVisible: boolean; openQueue: (trigger: HTMLElement) => void;
  } = $props();

  const mac = typeof navigator !== "undefined" && /Mac|iPad|iPhone/i.test(navigator.platform);
  const auth = getAuthStore();
  const name = $derived(auth.user?.display_name || auth.user?.username || t("nav.settings"));
  let accountOpen = $state(false);
  let brokenAvatar = $state<string>();
</script>

<header class="workspace-header">
  <div class="header-inner">
    {#if mobile}
      <a href="/" class="wordmark" data-workspace-home aria-label="SoundTime"><SoundTimeSignature width={118} /></a>
      <button id="directory-toggle" class="command" type="button" aria-label={t(menuOpen ? "workspace.hideMenu" : "workspace.showMenu")}
        aria-haspopup="dialog" aria-expanded={menuOpen} aria-controls={menuOpen ? "workspace-directory" : undefined} onclick={openMenu}>
        <Menu size={18} />
      </button>
    {/if}
    <button class="command search" type="button" aria-label={t("common.search")} aria-haspopup="dialog"
      aria-keyshortcuts="Meta+K Control+K" onclick={event => { event.currentTarget.focus(); openSearch(); }}>
      <Search size={17} /><span>{t("common.search")}</span><kbd>{mac ? "⌘K" : "Ctrl K"}</kbd>
    </button>
    <div class="tools">
      {#if mobile}
        <button class="command mobile-queue" type="button" aria-label={t("player.queue")} aria-haspopup="dialog" aria-expanded={queueVisible}
          aria-controls={queueVisible ? "workspace-queue" : undefined} onclick={event => openQueue(event.currentTarget)}><ListMusic size={19} /></button>
      {/if}
      {#if !auth.isAuthenticated}
        <a class="command sign-in" href={signInLink(page.url)} aria-label={t("nav.signIn")}><UserRound size={17} /><span>{t("nav.signIn")}</span></a>
      {/if}
      <Popover.Root bind:open={accountOpen}>
        <Popover.Trigger class={auth.isAuthenticated ? "account-trigger" : "icon-button"}
          aria-label={auth.isAuthenticated ? name : t("authAppearance.label")} title={auth.isAuthenticated ? name : t("authAppearance.label")}>
          {#if auth.isAuthenticated}
            <span class="account-name">{name}</span>
            <span class="avatar" aria-hidden="true">
              {#if auth.user?.avatar_url && brokenAvatar !== auth.user.avatar_url}
                <img src={auth.user.avatar_url} alt="" onerror={() => brokenAvatar = auth.user?.avatar_url ?? undefined} />
              {:else}{name.slice(0,1).toLocaleUpperCase()}{/if}
            </span>
            <ChevronDown size={12} class="account-chevron" />
          {:else}<SunMoon size={17} />{/if}
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content role="dialog" aria-label={auth.isAuthenticated ? name : t("authAppearance.label")} align="end" sideOffset={6} class="account-popover">
            {#if auth.isAuthenticated}
              <p class="account-title">{name}</p>
              {#if auth.user?.email}<p class="account-detail">{auth.user.email}</p>{/if}
              <a href="/settings" onclick={() => accountOpen = false}><Settings size={15} />{t("nav.settings")}</a>
            {/if}
            <div class="appearance-row"><span>{t("authAppearance.label")}</span><AppearanceSwitcher /></div>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </div>
  </div>
</header>

<style>
  .workspace-header { flex: none; min-width: 0; color: hsl(var(--foreground)); }
  .header-inner { display: flex; align-items: center; min-width: 0; gap: 9px; min-height: 71px; padding: 9px clamp(18px,3.4cqi,38px); }
  .wordmark { display: inline-flex; align-items: center; flex: none; min-height: 44px; }
  .tools, .command, :global(.workspace-header .account-trigger) { display: flex; align-items: center; }
  .tools { gap: 8px; margin-inline-start: auto; }
  .command { justify-content: center; gap: 7px; min-height: 35px; padding-inline: 9px; border-radius: 10px; color: hsl(var(--muted-foreground)); font-size: .82rem; font-weight: 520; white-space: nowrap; }
  .command:hover, :global(.workspace-header .account-trigger:hover) { background: hsl(var(--card) / .62); color: hsl(var(--foreground)); }
  .search { width: min(62%, 350px); justify-content: flex-start; padding-inline: 12px; background: hsl(var(--card) / .58); border: 1px solid var(--glass-edge); }
  kbd { margin-inline-start: auto; color: hsl(var(--muted-foreground)); font: inherit; font-size: .68rem; }
  :global(.workspace-header .icon-button) { display: grid; place-items: center; min-width: 34px; min-height: 36px; border-radius: 8px; }
  :global(.workspace-header .account-trigger) { min-height: 38px; gap: 6px; border-radius: 8px; padding: 2px 5px; font-size: .8rem; font-weight: 550; }
  .account-name { max-width: 13ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .avatar { display: grid; place-items: center; flex: none; width: 29px; height: 29px; border-radius: 50%; overflow: hidden; background: hsl(var(--secondary)); color: hsl(var(--secondary-foreground)); font-size: .77rem; font-weight: 650; }
  .avatar img { width: 100%; height: 100%; object-fit: cover; }
  :global(.account-chevron) { color: hsl(var(--muted-foreground)); }
  :global(.account-popover) { z-index: 60; min-width: 225px; max-width: min(90vw,20rem); border: 1px solid hsl(var(--border)); border-radius: 12px; padding: 11px; color: hsl(var(--popover-foreground)); background: hsl(var(--popover)); box-shadow: 0 12px 36px -12px rgb(0 0 0 / .22); }
  .account-title, .account-detail { padding-inline: 5px; overflow-wrap: anywhere; font-size: .84rem; font-weight: 600; }
  .account-detail { color: hsl(var(--muted-foreground)); font-size: .76rem; font-weight: 450; }
  :global(.account-popover) a { display: flex; align-items: center; gap: 8px; margin-block: 7px 3px; padding: 7px 5px; border-radius: 6px; font-size: .85rem; }
  :global(.account-popover) a:hover { background: hsl(var(--muted)); }
  .appearance-row { display: flex; gap: 8px; align-items: center; justify-content: space-between; padding-inline: 5px; font-size: .8rem; }
  a + .appearance-row { border-top: 1px solid hsl(var(--border)); padding-top: 10px; }
  @media (max-width: 767px) {
    .header-inner { gap: 2px; min-height: 54px; padding: 3px 8px 3px 13px; }
    .wordmark { width: clamp(95px,29vw,136px); min-width: 0; flex-shrink: 1; margin-inline-end: auto; }
    .tools { gap: 1px; margin: 0; }
    .search span, kbd, .sign-in span, .account-name, :global(.account-chevron) { display: none; }
    .search { width: auto; border: 0; background: transparent; }
    .command, :global(.workspace-header .icon-button), :global(.workspace-header .account-trigger) { padding: 0; min-width: 37px; min-height: 44px; justify-content: center; }
  }
</style>
