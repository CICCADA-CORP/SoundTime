<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import LoginAppearance from "./LoginAppearance.svelte";

  let { children, publicLinks }: { children: Snippet; publicLinks: boolean } = $props();
  let darkAppearance = $state(false);
</script>

<div class="account-surface" data-appearance={darkAppearance ? "dark" : "light"}>
  <a class="skip-link" href="#main-content">{t("shell.skipToContent")}</a>
  <header class="appearance"><LoginAppearance bind:dark={darkAppearance} /></header>
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
    --background: 0 0% 100%; --foreground: 210 8% 7%; --card: var(--background);
    --primary: 0 0% 6%; --primary-foreground: 0 0% 100%; --muted-foreground: 207 12% 34%;
    --sidebar: 210 7% 94.5%; --border: 210 9% 89%; --input: 212 11% 80%;
    --muted: 210 10% 95%; --accent: 210 8% 90%; --accent-foreground: var(--foreground);
    --account-link: 206 41% 32%; --ring: 209 33% 43%;
    color-scheme: light; display: grid; grid-template-rows: auto 1fr auto;
    min-height: 100svh; background: hsl(var(--sidebar)); color: hsl(var(--foreground));
  }
  .account-surface[data-appearance="dark"] {
    --background: 212 11% 12%; --foreground: 207 16% 94%; --primary: 207 16% 94%;
    --primary-foreground: 211 16% 10%; --muted-foreground: 211 10% 72%; --sidebar: 215 12% 8%;
    --border: 214 9% 22%; --input: 212 9% 31%; --muted: 211 10% 16%; --accent: 213 8% 22%;
    --account-link: 205 52% 76%; --ring: 205 40% 63%; --destructive: 4 90% 74%; color-scheme: dark;
  }
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
