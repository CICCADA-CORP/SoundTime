<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { ChevronRight, UserRound, X } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import SoundTimeSignature from "../SoundTimeSignature.svelte";
  import LibraryNavigation from "./LibraryNavigation.svelte";
  import { followSidebarLink, isActive } from "./navigation";

  let { mobile = false, close }: { mobile?: boolean; close?: () => void } = $props();
  const auth = getAuthStore();
  const accountRoute = $derived(auth.isAuthenticated ? "/settings" : signInLink(page.url));
  const displayName = $derived(auth.user?.display_name || auth.user?.username || t("workspaceNav.guest"));
</script>

<aside class="sidebar" class:mobile>
  <div class="masthead">
    <a href="/" class="brand" aria-label="SoundTime" onclick={(event) => followSidebarLink(event, close)}>
      <SoundTimeSignature width={135} />
    </a>
    {#if mobile}
      <button type="button" class="close" aria-label={t("a11y.close")} onclick={close}><X size={18} /></button>
    {/if}
  </div>

  <div class="scroll-region"><LibraryNavigation {close} /></div>

  <footer>
    <a href={accountRoute} class="account" aria-label={auth.isAuthenticated ? t("nav.settings") : t("nav.signIn")}
      aria-current={isActive(page.url.pathname, accountRoute) ? "page" : undefined} onclick={(event) => followSidebarLink(event, close)}>
      <span class="avatar" aria-hidden="true">
        {#if auth.isAuthenticated}{displayName.slice(0, 1).toLocaleUpperCase()}{:else}<UserRound size={16} strokeWidth={1.7} />{/if}
      </span>
      <span class="identity">
        <span class="name">{displayName}</span>
        <span class="detail">{auth.user?.email || (auth.isAuthenticated ? t("nav.settings") : t("nav.signIn"))}</span>
      </span>
      <ChevronRight size={15} strokeWidth={1.6} aria-hidden="true" />
    </a>
  </footer>
</aside>

<style>
  .sidebar {
    display: flex; flex-direction: column; height: 100%; width: 100%; min-height: 0; min-width: 0;
    color: hsl(var(--muted-foreground)); background: hsl(var(--sidebar)); border-right: 1px solid hsl(var(--border));
  }
  .masthead { flex: none; display: flex; align-items: center; justify-content: space-between; min-height: 54px; padding: 7px 16px; }
  .brand { display: inline-flex; align-items: center; color: hsl(var(--foreground)); }
  .scroll-region { flex: 1; min-height: 0; overflow-y: auto; overscroll-behavior-y: contain; scrollbar-width: thin; }
  .close { display: grid; place-items: center; width: 36px; height: 36px; border-radius: 8px; }
  .close:hover, .account:hover { background: hsl(var(--accent) / .55); }
  footer { flex: none; padding: 10px 8px max(10px, env(safe-area-inset-bottom)); }
  .account { display: flex; gap: 11px; align-items: center; min-height: 47px; border-radius: 8px; padding: 6px 8px; }
  .account[aria-current] { background: hsl(var(--accent)); }
  .avatar {
    display: grid; place-items: center; flex: none; width: 32px; height: 32px; color: hsl(var(--primary-foreground));
    background: hsl(var(--primary)); border-radius: 50%; font-size: 14px; font-weight: 600;
  }
  .identity { display: grid; min-width: 0; flex: 1; gap: 1px; }
  .name, .detail { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; line-height: 1.3; }
  .name { color: hsl(var(--foreground)); font-size: 13.5px; font-weight: 540; }
  .detail { color: hsl(var(--muted-foreground)); font-size: 12px; }
  .account :global(svg) { flex: none; }
  .mobile { border-right: none; }
  .mobile .masthead { min-height: 60px; }
</style>
