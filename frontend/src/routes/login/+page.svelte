<script lang="ts">
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { goto } from "$app/navigation";
  import { t } from "$lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import { api } from "$lib/api";

  const auth = getAuthStore();
  let username = $state("");
  let password = $state("");
  let error = $state("");
  let loading = $state(false);
  let registrationOpen = $state(false);

  onMount(async () => {
    try {
      const info = await api.get<{ open_registration: boolean }>("/nodeinfo");
      registrationOpen = info.open_registration;
    } catch {
      registrationOpen = false;
    }
  });

  async function handleLogin() {
    error = "";
    loading = true;
    try {
      await auth.login(username, password);
      goto("/");
    } catch (e) {
      error = e instanceof Error ? e.message : "Login failed";
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>Sign In — SoundTime</title></svelte:head>

<div class="min-h-[70vh] flex items-center justify-center">
  <div class="w-full max-w-sm space-y-6">
    <div class="text-center">
      <h1 class="text-3xl font-bold">{t('auth.login')}</h1>
      <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">{t('auth.loginSubtitle')}</p>
    </div>

    <form class="space-y-4" onsubmit={(e) => { e.preventDefault(); handleLogin(); }}>
      <div>
        <label for="username" class="text-sm font-medium block mb-1">{t('auth.username')}</label>
        <input id="username" type="text" bind:value={username} required class="w-full px-4 py-2.5 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
      </div>

      <div>
        <label for="password" class="text-sm font-medium block mb-1">{t('auth.password')}</label>
        <input id="password" type="password" bind:value={password} required class="w-full px-4 py-2.5 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
      </div>

      {#if error}
        <p class="text-sm text-red-400">{error}</p>
      {/if}

      <button type="submit" disabled={loading} class="w-full py-2.5 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium hover:opacity-90 disabled:opacity-50 transition">
        {loading ? t('auth.loggingIn') : t('auth.login')}
      </button>
    </form>

    {#if registrationOpen}
    <p class="text-center text-sm text-[hsl(var(--muted-foreground))]">
      {t('auth.noAccount')}
      <a href="/register" class="text-[hsl(var(--primary))] hover:underline">{t('auth.signUp')}</a>
    </p>
    {/if}
  </div>
</div>
