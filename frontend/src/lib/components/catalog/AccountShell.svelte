<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import AppearanceSwitcher from "$lib/components/ui/appearance-switcher.svelte";

  let { children, publicLinks }: { children: Snippet; publicLinks: boolean } = $props();
</script>

<div class="account-surface">
  <a class="skip-link" href="#main-content">{t("shell.skipToContent")}</a>
  <header class="appearance"><AppearanceSwitcher /></header>
  <main id="main-content" tabindex="-1">{@render children()}</main>
  <footer>
    {#if publicLinks}
      <a href="/">{t("nav.home")}</a>
      <span aria-hidden="true">·</span>
      <a href="/tos">{t("authShell.terms")}</a>
    {/if}
  </footer>
</div>

<style>
  .account-surface {
    --account-link: var(--link, var(--primary)); display: grid; grid-template-rows: auto 1fr auto;
    min-height: 100svh; color: hsl(var(--foreground));
  }
  footer a { padding: .18rem .35rem; }
  .appearance { display: flex; align-items: flex-start; justify-content: flex-end; padding: 16px max(16px, env(safe-area-inset-right)) 0; }
  main { min-width: 0; display: grid; place-items: center; padding: 24px 24px 32px; }
  footer { display: flex; min-height: 66px; align-items: flex-start; justify-content: center; gap: 12px; padding: 10px 16px max(24px, env(safe-area-inset-bottom)); font-size: 12px; color: hsl(var(--muted-foreground)); }
  footer a:hover { color: hsl(var(--foreground)); text-decoration: underline; }
  @media (max-width: 680px) {
    .appearance { padding: max(12px, env(safe-area-inset-top)) 14px 0; }
    main { padding: 16px 13px; }
    footer { min-height: 48px; }
  }
</style>
