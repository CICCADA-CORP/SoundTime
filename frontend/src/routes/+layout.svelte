<script lang="ts">
  import "../app.css";
  import { afterNavigate, goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, type Snippet } from "svelte";
  import AccountShell from "$lib/components/catalog/AccountShell.svelte";
  import { accessRedirect, isSetupStatus } from "$lib/access/navigation";
  import InstanceUnavailable from "$lib/components/catalog/InstanceUnavailable.svelte";
  import AppShell from "$lib/components/shell/AppShell.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getTaskStore } from "$lib/stores/tasks.svelte";
  import { getThemeStore } from "$lib/stores/theme.svelte";
  import { api } from "$lib/api";
  import type { SetupStatus } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";

  let { children }: { children: Snippet } = $props();
  const auth = getAuthStore();
  const tasks = getTaskStore();
  const theme = getThemeStore();
  let loading = $state(true);
  let status = $state<SetupStatus | null>(null);
  let request = 0;
  const onSetup = $derived(page.url.pathname === "/setup");
  const onAccount = $derived(["/login", "/register"].includes(page.url.pathname));
  const redirect = $derived(status && accessRedirect(status, page.url, auth.isAuthenticated));

  async function initialize() {
    const current = ++request;
    loading = true;
    const [result, session] = await Promise.allSettled([
      api.get<unknown>("/setup/status"),
      auth.init(),
    ]);
    if (current !== request) return;
    status = session.status === "fulfilled" && result.status === "fulfilled" && isSetupStatus(result.value) ? result.value : null;
    loading = false;
  }
  onMount(() => {
    void initialize();
    void theme.init();
  });
  afterNavigate(({ from, to }) => {
    if (from?.url.pathname === "/setup" && to?.url.pathname !== "/setup"
      || !status && from && ["/login", "/register"].includes(from.url.pathname)) void initialize();
  });
  $effect(() => {
    if (!loading && redirect) void goto(redirect, { replaceState: true });
  });
  $effect(() => {
    if (auth.isAdmin) void tasks.checkForRunningTask();
  });
</script>

{#if loading || redirect}
  <div class="grid min-h-screen place-items-center bg-background" role="status" aria-label={t("common.loading")}>
    <img src="/soundtime-logo.svg" alt="" width="42" height="42" />
  </div>
{:else if onAccount}
  <AccountShell publicLinks={!!status && (!status.instance_private || auth.isAuthenticated)}>{@render children()}</AccountShell>
{:else if !status}
  <InstanceUnavailable retry={initialize} />
{:else if onSetup}
  {@render children()}
{:else}
  <AppShell>{@render children()}</AppShell>
{/if}
