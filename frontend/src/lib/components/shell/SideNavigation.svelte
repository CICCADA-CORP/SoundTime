<script lang="ts">
  import { page } from "$app/state";
  import { Settings, UserRound, X } from "lucide-svelte";
  import { signInLink } from "$lib/access/navigation";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import SoundTimeSignature from "../SoundTimeSignature.svelte";
  import LibraryNavigation from "./LibraryNavigation.svelte";
  import { followSidebarLink, isActive } from "./navigation";

  let { close, rail = false }: { close?: () => void; rail?: boolean } = $props();
  const auth = getAuthStore();
  const accountRoute = $derived(auth.isAuthenticated ? "/settings" : signInLink(page.url));
</script>

<div class="directory" class:rail>
  <div class="masthead">
    {#if rail}
      <a href="/" class="brand" data-workspace-home aria-label="SoundTime">
        <span class="signature"><SoundTimeSignature width={126} /></span>
        <img class="mark" src="/soundtime-logo.svg" alt="" width="32" height="32" />
      </a>
    {:else}<h2>{t("shell.navigation")}</h2>{/if}
    {#if close}<button type="button" aria-label={t("a11y.close")} onclick={close}><X size={20} /></button>{/if}
  </div>
  <div class="scroll-region"><LibraryNavigation {close} {rail} /></div>
  <footer>
    <a href={accountRoute} aria-label={t(auth.isAuthenticated ? "nav.settings" : "nav.signIn")} title={t(auth.isAuthenticated ? "nav.settings" : "nav.signIn")} aria-current={isActive(page.url.pathname, accountRoute) ? "page" : undefined} onclick={event => followSidebarLink(event, close)}>
      {#if auth.isAuthenticated}<Settings size={16} />{:else}<UserRound size={16} />{/if}<span>{t(auth.isAuthenticated ? "nav.settings" : "nav.signIn")}</span>
    </a>
  </footer>
</div>

<style>
  .directory { display: flex; flex-direction: column; height: 100%; padding: 0 clamp(16px, 4vw, 30px); }
  .masthead, footer a { display: flex; align-items: center; justify-content: space-between; }
  .masthead { gap: 16px; min-height: 67px; border-bottom: 1px solid hsl(var(--border)); }
  h2 { font-weight: 620; font-size: 1.35rem; line-height: 1.18; letter-spacing: -.033em; }
  .masthead button { display: grid; place-items: center; width: 36px; height: 36px; border-radius: 50%; background: hsl(var(--muted)); color: hsl(var(--muted-foreground)); }
  .scroll-region { min-height: 0; overflow: auto; flex: 1; overscroll-behavior: contain; }
  footer { border-top: 1px solid hsl(var(--border)); }
  footer a { min-height: 46px; font-size: .85rem; font-weight: 600; }
  .masthead button:hover, footer a:hover { color: hsl(var(--primary)); }
  footer a { justify-content: flex-start; gap: 10px; }
  .rail { padding: 0 12px 5px; }
  .rail .masthead { min-height: 71px; padding-inline: 9px; border: 0; }
  .brand { display: inline-flex; align-items: center; min-height: 40px; }
  .mark { display: none; }
  .rail footer { margin-inline: 3px; border-color: var(--glass-edge); }
  .rail footer a { padding-inline: 7px; font-size: .78rem; font-weight: 530; }
  @media (min-width: 768px) and (max-width: 1000px) {
    .rail { padding-inline: 7px; }
    .rail .masthead, .rail footer a { justify-content: center; padding: 0; }
    .rail .mark { display: block; }
    .rail .signature, .rail footer span { display: none; }
  }
</style>
