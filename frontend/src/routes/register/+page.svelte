<script lang="ts">
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { goto } from "$app/navigation";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  let email = "";
  let username = "";
  let password = "";
  let confirmPassword = "";
  let error = "";
  let loading = false;

  async function handleRegister() {
    error = "";
    if (password !== confirmPassword) {
      error = "Passwords don't match";
      return;
    }
    if (password.length < 8) {
      error = "Password must be at least 8 characters";
      return;
    }
    loading = true;
    try {
      await auth.register(email, username, password);
      goto("/");
    } catch (e) {
      error = e instanceof Error ? e.message : "Registration failed";
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>Sign Up — SoundTime</title></svelte:head>

<div class="min-h-[70vh] flex items-center justify-center">
  <div class="w-full max-w-sm space-y-6">
    <div class="text-center">
      <h1 class="text-3xl font-bold">{t('auth.register')}</h1>
      <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">{t('auth.registerSubtitle')}</p>
    </div>

    <form class="space-y-4" on:submit|preventDefault={handleRegister}>
      <div>
        <label for="email" class="text-sm font-medium block mb-1">{t('auth.email')}</label>
        <input id="email" type="email" bind:value={email} required class="w-full px-4 py-2.5 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
      </div>

      <div>
        <label for="username" class="text-sm font-medium block mb-1">{t('auth.username')}</label>
        <input id="username" type="text" bind:value={username} required class="w-full px-4 py-2.5 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
      </div>

      <div>
        <label for="password" class="text-sm font-medium block mb-1">{t('auth.password')}</label>
        <input id="password" type="password" bind:value={password} required minlength="8" class="w-full px-4 py-2.5 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
      </div>

      <div>
        <label for="confirm" class="text-sm font-medium block mb-1">{t('auth.confirmPassword')}</label>
        <input id="confirm" type="password" bind:value={confirmPassword} required class="w-full px-4 py-2.5 rounded-lg bg-[hsl(var(--secondary))] text-sm border border-[hsl(var(--border))] outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
      </div>

      {#if error}
        <p class="text-sm text-red-400">{error}</p>
      {/if}

      <button type="submit" disabled={loading} class="w-full py-2.5 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium hover:opacity-90 disabled:opacity-50 transition">
        {loading ? t('auth.registering') : t('auth.register')}
      </button>
    </form>

    <p class="text-center text-sm text-[hsl(var(--muted-foreground))]">
      {t('auth.hasAccount')}
      <a href="/login" class="text-[hsl(var(--primary))] hover:underline">{t('auth.signIn')}</a>
    </p>
  </div>
</div>
