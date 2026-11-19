<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import { accountLink } from "$lib/access/navigation";
  import { goto } from "$app/navigation";
  import SoundTimeSignature from "$lib/components/SoundTimeSignature.svelte";
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
  let statusFailed = $state(false);
  let redirectTimer: ReturnType<typeof setTimeout>;
  let adminLoginRequired = $state(false);
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
  let instancePrivate = $state(false);
  let openRegistrations = $state(true);
  let maxUploadSizeMb = $state(500);

  async function checkSetup() {
    loading = true;
    statusFailed = false;
    try {
      const status = await api.get<SetupStatus>("/setup/status");
      if (status.setup_complete) {
        await goto("/", { replaceState: true });
        return;
      }
      instancePrivate = status.instance_private;
      if (status.has_admin) {
        adminLoginRequired = !auth.isAdmin;
        if (auth.isAdmin) currentStep = 2;
      }
    } catch {
      statusFailed = true;
    } finally {
      loading = false;
    }
  }
  onMount(() => { void checkSetup(); });
  onDestroy(() => clearTimeout(redirectTimer));

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
        instance_private: instancePrivate,
        open_registrations: openRegistrations,
        max_upload_size_mb: maxUploadSizeMb,
      } satisfies SetupCompleteRequest);
      currentStep = 4;
      // Short delay then redirect
      redirectTimer = setTimeout(() => goto("/"), 2000);
    } catch (e: unknown) {
      error = (e instanceof Error ? e.message : String(e)) || t('setup.finalizingError');
    }
    submitting = false;
  }

  const steps = $derived([
    { num: 1, label: t('setup.stepAdmin') },
    { num: 2, label: t('setup.stepInstance') },
    { num: 3, label: t('setup.stepP2P') },
    { num: 4, label: t('setup.stepDone') },
  ]);
</script>

{#if loading}
  <div class="flex items-center justify-center min-h-screen">
    <div class="motion-safe:animate-spin w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full"></div>
  </div>
{:else if statusFailed || adminLoginRequired}
  <div class="mx-auto max-w-lg space-y-4 px-6 py-12">
    <h1 class="text-xl font-semibold">{t(statusFailed ? "access.unavailable" : "access.resumeSetup")}</h1>
    {#if statusFailed}<Button onclick={checkSetup}>{t("access.retry")}</Button>
    {:else}<Button href={accountLink("/login", "/setup")}>{t("auth.signIn")}</Button>{/if}
  </div>
{:else}
  <div class="my-6 w-full max-w-lg rounded-xl border border-border bg-card p-5 shadow-sm sm:p-8">
    <!-- Logo -->
    <div class="text-center mb-8">
      <h1 class="mb-3 flex justify-center">
        <SoundTimeSignature width={214} />
      </h1>
      <p class="text-[hsl(var(--muted-foreground))] mt-1">{t('setup.subtitle')}</p>
    </div>

    <!-- Progress bar -->
    <div class="flex items-center justify-between mb-10 px-2">
      {#each steps as step, i (step.num)}
        <div class="flex items-center {i < steps.length - 1 ? 'flex-1' : ''}">
          <div class="flex flex-col items-center">
            <div
              class="w-9 h-9 rounded-full flex items-center justify-center text-sm font-medium transition-colors
                {currentStep >= step.num ? 'bg-primary text-primary-foreground' : 'bg-secondary text-muted-foreground'}"
            >
              {#if currentStep > step.num}
                ✓
              {:else}
                {step.num}
              {/if}
            </div>
            <span class="text-xs mt-1.5 text-[hsl(var(--muted-foreground))] text-center">{step.label}</span>
          </div>
          {#if i < steps.length - 1}
            <div class="flex-1 h-0.5 mx-3 mt-[-1rem] {currentStep > step.num ? 'bg-primary' : 'bg-[hsl(var(--secondary))]'}"></div>
          {/if}
        </div>
      {/each}
    </div>

    <!-- Error -->
    {#if error}
      <div role="alert" class="mb-4 rounded-md border border-destructive/30 p-3 text-sm text-destructive">
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
            <Input
              id="username" autocomplete="username"
              type="text"
              bind:value={adminUsername}
              required
              minlength={3}
              maxlength={64}
              class="h-10 bg-background shadow-none"
              placeholder="admin"
            />
          </div>

          <div>
            <label for="email" class="block text-sm font-medium mb-1.5">{t('setup.emailLabel')}</label>
            <Input
              id="email"
              type="email" autocomplete="email"
              bind:value={adminEmail}
              required
              class="h-10 bg-background shadow-none"
              placeholder="admin@example.com"
            />
          </div>

          <div>
            <label for="password" class="block text-sm font-medium mb-1.5">{t('setup.passwordLabel')}</label>
            <Input
              id="password"
              type="password" autocomplete="new-password"
              bind:value={adminPassword}
              required
              minlength={8}
              class="h-10 bg-background shadow-none"
              placeholder="••••••••"
            />
          </div>

          <div>
            <label for="password-confirm" class="block text-sm font-medium mb-1.5">{t('setup.confirmPasswordLabel')}</label>
            <Input
              id="password-confirm"
              type="password" autocomplete="new-password"
              bind:value={adminPasswordConfirm}
              required
              minlength={8}
              class="h-10 bg-background shadow-none"
              placeholder="••••••••"
            />
          </div>

          <button
            type="submit"
            disabled={submitting}
            class="w-full py-2.5 bg-primary text-primary-foreground rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
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
            <Input
              id="instance-name"
              type="text"
              bind:value={instanceName}
              required
              class="h-10 bg-background shadow-none"
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
              type="submit"
              disabled={submitting}
              class="flex-1 py-2.5 bg-primary text-primary-foreground rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
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
                aria-label={t("setup.p2pLabel")}
                bind:checked={p2pEnabled}
                class="sr-only peer"
              />
              <div class="w-11 h-6 bg-[hsl(var(--border))] peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-[hsl(var(--primary))]"></div>
            </label>
          </div>

          <label class="flex items-center justify-between gap-4 rounded-lg border border-border bg-secondary p-4">
            <span><strong class="block text-sm font-medium">{t("admin.settings.privateInstance")}</strong>
              <span class="block text-xs text-muted-foreground">{t("admin.settings.privateInstanceDesc")}</span>
            </span>
            <input type="checkbox" class="size-5 accent-primary" bind:checked={instancePrivate} />
          </label>

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
                aria-label={t("setup.openRegistrations")}
                checked={!instancePrivate && openRegistrations}
                disabled={instancePrivate}
                onchange={(event) => openRegistrations = event.currentTarget.checked}
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
              <span class="text-sm font-mono text-[hsl(var(--primary))]">{maxUploadSizeMb} {t('common.megabytes')}</span>
            </div>
            <input
              type="range"
              aria-label={t("setup.maxUploadSize")}
              bind:value={maxUploadSizeMb}
              min="50"
              max="2000"
              step="50"
              class="w-full h-2 bg-[hsl(var(--border))] rounded-lg appearance-none cursor-pointer accent-[hsl(var(--primary))]"
            />
            <div class="flex justify-between text-xs text-[hsl(var(--muted-foreground))] mt-1">
              <span>50 {t('common.megabytes')}</span>
              <span>2 {t('common.gigabytes')}</span>
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
            class="flex-1 py-2.5 bg-primary text-primary-foreground rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
          >
            {#if submitting}
              {t('setup.finalizing')}
            {:else}
              {t('setup.finish')}
            {/if}
          </button>
        </div>
      </div>
    {/if}

    <!-- Step 4: Complete -->
    {#if currentStep === 4}
      <div class="text-center space-y-6">
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
            <span class="font-medium">{auth.user?.username ?? adminUsername}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryInstance')}</span>
            <span class="font-medium">{instanceName}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryP2P')}</span>
            <span class="font-medium">{p2pEnabled ? t('setup.enabled') : t('setup.disabled')}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryRegistrations')}</span>
            <span class="font-medium">{!instancePrivate && openRegistrations ? t('setup.open') : t('setup.closed')}</span>

            <span class="text-[hsl(var(--muted-foreground))]">{t('setup.summaryMaxUpload')}</span>
            <span class="font-medium">{maxUploadSizeMb} {t('common.megabytes')}</span>
          </div>
        </div>

        <div class="motion-safe:animate-spin w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full mx-auto"></div>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('setup.redirecting')}</p>
      </div>
    {/if}
  </div>
{/if}
