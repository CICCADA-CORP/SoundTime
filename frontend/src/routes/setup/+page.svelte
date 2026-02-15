<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api, setTokens } from "$lib/api";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import type {
    SetupStatus,
    AuthResponse,
    SetupInstanceRequest,
    SetupCompleteRequest,
  } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();

  let currentStep = $state(1);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state("");

  // Step 1: Admin
  let adminUsername = $state("");
  let adminEmail = $state("");
  let adminPassword = $state("");
  let adminPasswordConfirm = $state("");

  // Step 2: Instance
  let instanceName = $state("SoundTime");
  let instanceDescription = $state("A P2P music streaming instance");

  // Step 3: P2P & Registrations
  let p2pEnabled = $state(true);
  let openRegistrations = $state(true);
  let maxUploadSizeMb = $state(500);

  onMount(async () => {
    try {
      const status = await api.get<SetupStatus>("/setup/status");
      if (status.setup_complete) {
        goto("/");
        return;
      }
      if (status.has_admin) {
        // Admin already created, skip to step 2
        // But need to be logged in — check auth
        await auth.init();
        if (auth.isAuthenticated && auth.isAdmin) {
          currentStep = 2;
        } else {
          // Admin exists but we're not logged in as admin — show login prompt
          currentStep = 1;
        }
      }
    } catch {
      // API not available yet, show step 1
    }
    loading = false;
  });

  async function handleCreateAdmin() {
    error = "";
    if (adminUsername.length < 3) {
      error = t('setup.usernameMinLength');
      return;
    }
    if (adminPassword.length < 8) {
      error = t('setup.passwordMinLength');
      return;
    }
    if (adminPassword !== adminPasswordConfirm) {
      error = t('setup.passwordMismatch');
      return;
    }

    submitting = true;
    try {
      const data = await api.post<AuthResponse>("/setup/admin", {
        username: adminUsername,
        email: adminEmail,
        password: adminPassword,
      });
      setTokens(data.tokens.access_token, data.tokens.refresh_token);
      await auth.init();
      currentStep = 2;
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t('setup.adminCreateError');
    }
    submitting = false;
  }

  async function handleConfigureInstance() {
    error = "";
    if (!instanceName.trim()) {
      error = t('setup.instanceNameRequired');
      return;
    }
    submitting = true;
    try {
      await api.post("/setup/instance", {
        instance_name: instanceName,
        instance_description: instanceDescription,
      } satisfies SetupInstanceRequest);
      currentStep = 3;
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t('setup.configureError');
    }
    submitting = false;
  }

  async function handleComplete() {
    error = "";
    submitting = true;
    try {
      await api.post("/setup/complete", {
        p2p_enabled: p2pEnabled,
        open_registrations: openRegistrations,
        max_upload_size_mb: maxUploadSizeMb,
      } satisfies SetupCompleteRequest);
      currentStep = 4;
      // Short delay then redirect
      setTimeout(() => goto("/"), 2000);
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t('setup.finalizingError');
    }
    submitting = false;
  }

  const steps = [
    { num: 1, label: t('setup.stepAdmin') },
    { num: 2, label: t('setup.stepInstance') },
    { num: 3, label: t('setup.stepP2P') },
    { num: 4, label: t('setup.stepDone') },
  ];
</script>

{#if loading}
  <div class="flex items-center justify-center min-h-screen">
    <div class="animate-spin w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full"></div>
  </div>
{:else}
  <div class="w-full max-w-lg mx-auto px-6 py-12">
    <!-- Logo -->
    <div class="text-center mb-8">
      <div class="text-5xl mb-3">🎵</div>
      <h1 class="text-3xl font-bold">SoundTime</h1>
      <p class="text-[hsl(var(--muted-foreground))] mt-1">{t('setup.subtitle')}</p>
    </div>

    <!-- Progress bar -->
    <div class="flex items-center justify-between mb-10 px-2">
      {#each steps as step, i}
        <div class="flex items-center {i < steps.length - 1 ? 'flex-1' : ''}">
          <div class="flex flex-col items-center">
            <div
              class="w-9 h-9 rounded-full flex items-center justify-center text-sm font-medium transition-colors
                {currentStep > step.num ? 'bg-green-500 text-white' : currentStep === step.num ? 'bg-[hsl(var(--primary))] text-white' : 'bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))]'}"
            >
              {#if currentStep > step.num}
                ✓
              {:else}
                {step.num}
              {/if}
            </div>
            <span class="text-xs mt-1.5 text-[hsl(var(--muted-foreground))] whitespace-nowrap">{step.label}</span>
          </div>
          {#if i < steps.length - 1}
            <div class="flex-1 h-0.5 mx-3 mt-[-1rem] {currentStep > step.num ? 'bg-green-500' : 'bg-[hsl(var(--secondary))]'}"></div>
          {/if}
        </div>
      {/each}
    </div>

    <!-- Error -->
    {#if error}
      <div class="mb-4 p-3 rounded-md bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
        {error}
      </div>
    {/if}

    <!-- Step 1: Admin Account -->
    {#if currentStep === 1}
      <div class="space-y-6">
        <div>
          <h2 class="text-xl font-semibold mb-1">{t('setup.createAdmin')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            {t('setup.adminDescription')}
          </p>
        </div>

        <form onsubmit={(e) => { e.preventDefault(); handleCreateAdmin(); }} class="space-y-4">
          <div>
            <label for="username" class="block text-sm font-medium mb-1.5">{t('setup.usernameLabel')}</label>
            <input
              id="username"
              type="text"
              bind:value={adminUsername}
              required
              minlength="3"
              maxlength="64"
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              placeholder="admin"
            />
          </div>

          <div>
            <label for="email" class="block text-sm font-medium mb-1.5">{t('setup.emailLabel')}</label>
            <input
              id="email"
              type="email"
              bind:value={adminEmail}
              required
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              placeholder="admin@example.com"
            />
          </div>

          <div>
            <label for="password" class="block text-sm font-medium mb-1.5">{t('setup.passwordLabel')}</label>
            <input
              id="password"
              type="password"
              bind:value={adminPassword}
              required
              minlength="8"
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              placeholder="••••••••"
            />
          </div>

          <div>
            <label for="password-confirm" class="block text-sm font-medium mb-1.5">{t('setup.confirmPasswordLabel')}</label>
            <input
              id="password-confirm"
              type="password"
              bind:value={adminPasswordConfirm}
              required
              minlength="8"
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              placeholder="••••••••"
            />
          </div>

          <button
            type="submit"
            disabled={submitting}
            class="w-full py-2.5 bg-[hsl(var(--primary))] text-white rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
          >
            {#if submitting}
              {t('setup.creating')}
            {:else}
              {t('setup.createAdminButton')}
            {/if}
          </button>
        </form>
      </div>
    {/if}

    <!-- Step 2: Instance Configuration -->
    {#if currentStep === 2}
      <div class="space-y-6">
        <div>
          <h2 class="text-xl font-semibold mb-1">{t('setup.configureInstance')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            {t('setup.configureInstanceDesc')}
          </p>
        </div>

        <form onsubmit={(e) => { e.preventDefault(); handleConfigureInstance(); }} class="space-y-4">
          <div>
            <label for="instance-name" class="block text-sm font-medium mb-1.5">{t('setup.instanceNameLabel')}</label>
            <input
              id="instance-name"
              type="text"
              bind:value={instanceName}
              required
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              placeholder="My SoundTime"
            />
          </div>

          <div>
            <label for="instance-desc" class="block text-sm font-medium mb-1.5">{t('setup.instanceDescLabel')}</label>
            <textarea
              id="instance-desc"
              bind:value={instanceDescription}
              rows="3"
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))] resize-none"
              placeholder={t('setup.instanceDescPlaceholder')}
            ></textarea>
          </div>

          <div class="flex gap-3">
            <button
              type="button"
              onclick={() => { currentStep = 1; error = ''; }}
              class="flex-1 py-2.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-md font-medium hover:opacity-90 transition"
            >
              {t('setup.back')}
            </button>
            <button
              type="submit"
              disabled={submitting}
              class="flex-1 py-2.5 bg-[hsl(var(--primary))] text-white rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
            >
              {#if submitting}
                {t('setup.saving')}
              {:else}
                {t('setup.next')}
              {/if}
            </button>
          </div>
        </form>
      </div>
    {/if}

    <!-- Step 3: P2P & Registrations -->
    {#if currentStep === 3}
      <div class="space-y-6">
        <div>
          <h2 class="text-xl font-semibold mb-1">{t('setup.p2pTitle')}</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            {t('setup.p2pDescription')}
          </p>
        </div>

        <div class="space-y-5">
          <!-- P2P toggle -->
          <div class="flex items-center justify-between p-4 bg-[hsl(var(--secondary))] rounded-lg border border-[hsl(var(--border))]">
            <div>
              <p class="text-sm font-medium">{t('setup.p2pLabel')}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                {t('setup.p2pHint')}
              </p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer" for="p2p-toggle">
              <input
                id="p2p-toggle"
                type="checkbox"
                bind:checked={p2pEnabled}
                class="sr-only peer"
              />
              <div class="w-11 h-6 bg-[hsl(var(--border))] peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-[hsl(var(--primary))]"></div>
            </label>
          </div>

          <!-- Open registrations toggle -->
          <div class="flex items-center justify-between p-4 bg-[hsl(var(--secondary))] rounded-lg border border-[hsl(var(--border))]">
            <div>
              <p class="text-sm font-medium">{t('setup.openRegistrations')}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                {t('setup.openRegistrationsHint')}
              </p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer" for="registrations-toggle">
              <input
                id="registrations-toggle"
                type="checkbox"
                bind:checked={openRegistrations}
                class="sr-only peer"
              />
              <div class="w-11 h-6 bg-[hsl(var(--border))] peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-[hsl(var(--primary))]"></div>
            </label>
          </div>

          <!-- Max upload size -->
          <div class="p-4 bg-[hsl(var(--secondary))] rounded-lg border border-[hsl(var(--border))]">
            <div class="flex items-center justify-between mb-2">
              <div>
                <p class="text-sm font-medium">{t('setup.maxUploadSize')}</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                  {t('setup.maxUploadSizeHint')}
                </p>
              </div>
              <span class="text-sm font-mono text-[hsl(var(--primary))]">{maxUploadSizeMb} Mo</span>
            </div>
            <input
              type="range"
              bind:value={maxUploadSizeMb}
              min="50"
              max="2000"
              step="50"
              class="w-full h-2 bg-[hsl(var(--border))] rounded-lg appearance-none cursor-pointer accent-[hsl(var(--primary))]"
            />
            <div class="flex justify-between text-xs text-[hsl(var(--muted-foreground))] mt-1">
              <span>50 Mo</span>
              <span>2 Go</span>
            </div>
          </div>
        </div>

        <div class="flex gap-3 mt-6">
          <button
            type="button"
            onclick={() => { currentStep = 2; error = ''; }}
            class="flex-1 py-2.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-md font-medium hover:opacity-90 transition"
          >
            {t('setup.back')}
          </button>
          <button
            type="button"
            onclick={handleComplete}
            disabled={submitting}
            class="flex-1 py-2.5 bg-[hsl(var(--primary))] text-white rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
          >
            {#if submitting}
              {t('setup.finalizing')}
            {:else}
              {t('setup.next')}
            {/if}
          </button>
        </div>
      </div>
    {/if}

    <!-- Step 4: Complete -->
    {#if currentStep === 4}
      <div class="text-center space-y-6">
        <div class="text-6xl">🎉</div>
        <div>
          <h2 class="text-2xl font-bold">{t('setup.readyTitle')}</h2>
          <p class="text-[hsl(var(--muted-foreground))] mt-2">
            {t('setup.readyDescription')}
          </p>
        </div>

        <div class="bg-[hsl(var(--secondary))] rounded-lg p-5 text-left space-y-3 border border-[hsl(var(--border))]">
          <h3 class="text-sm font-semibold text-[hsl(var(--muted-foreground))] uppercase tracking-wide">{t('setup.summary')}</h3>
          <div class="grid grid-cols-2 gap-y-2 text-sm">
            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryAdmin')}</span>
            <span class="font-medium">{adminUsername}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryInstance')}</span>
            <span class="font-medium">{instanceName}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryP2P')}</span>
            <span class="font-medium">{p2pEnabled ? t('setup.enabled') : t('setup.disabled')}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryRegistrations')}</span>
            <span class="font-medium">{openRegistrations ? t('setup.open') : t('setup.closed')}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryMaxUpload')}</span>
            <span class="font-medium">{maxUploadSizeMb} Mo</span>
          </div>
        </div>

        <div class="animate-spin w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full mx-auto"></div>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('setup.redirecting')}</p>
      </div>
    {/if}
  </div>
{/if}
