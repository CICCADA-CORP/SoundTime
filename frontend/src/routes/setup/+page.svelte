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

  const auth = getAuthStore();

  let currentStep = 1;
  let loading = true;
  let submitting = false;
  let error = "";

  // Step 1: Admin
  let adminUsername = "";
  let adminEmail = "";
  let adminPassword = "";
  let adminPasswordConfirm = "";

  // Step 2: Instance
  let instanceName = "SoundTime";
  let instanceDescription = "A P2P music streaming instance";

  // Step 3: P2P & Registrations
  let p2pEnabled = true;
  let openRegistrations = true;
  let maxUploadSizeMb = 500;

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
      error = "Le nom d'utilisateur doit contenir au moins 3 caractères";
      return;
    }
    if (adminPassword.length < 8) {
      error = "Le mot de passe doit contenir au moins 8 caractères";
      return;
    }
    if (adminPassword !== adminPasswordConfirm) {
      error = "Les mots de passe ne correspondent pas";
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
    } catch (e: any) {
      error = e.message || "Erreur lors de la création du compte admin";
    }
    submitting = false;
  }

  async function handleConfigureInstance() {
    error = "";
    if (!instanceName.trim()) {
      error = "Le nom de l'instance est requis";
      return;
    }
    submitting = true;
    try {
      await api.post("/setup/instance", {
        instance_name: instanceName,
        instance_description: instanceDescription,
      } satisfies SetupInstanceRequest);
      currentStep = 3;
    } catch (e: any) {
      error = e.message || "Erreur lors de la configuration";
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
    } catch (e: any) {
      error = e.message || "Erreur lors de la finalisation";
    }
    submitting = false;
  }

  const steps = [
    { num: 1, label: "Compte admin" },
    { num: 2, label: "Instance" },
    { num: 3, label: "Réseau P2P" },
    { num: 4, label: "Terminé" },
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
      <p class="text-[hsl(var(--muted-foreground))] mt-1">Configuration de votre instance</p>
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
          <h2 class="text-xl font-semibold mb-1">Créer le compte administrateur</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            Ce compte aura un accès complet à la gestion de l'instance.
          </p>
        </div>

        <form on:submit|preventDefault={handleCreateAdmin} class="space-y-4">
          <div>
            <label for="username" class="block text-sm font-medium mb-1.5">Nom d'utilisateur</label>
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
            <label for="email" class="block text-sm font-medium mb-1.5">Adresse email</label>
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
            <label for="password" class="block text-sm font-medium mb-1.5">Mot de passe</label>
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
            <label for="password-confirm" class="block text-sm font-medium mb-1.5">Confirmer le mot de passe</label>
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
              Création en cours...
            {:else}
              Créer le compte admin
            {/if}
          </button>
        </form>
      </div>
    {/if}

    <!-- Step 2: Instance Configuration -->
    {#if currentStep === 2}
      <div class="space-y-6">
        <div>
          <h2 class="text-xl font-semibold mb-1">Configurer votre instance</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            Donnez un nom et une description à votre instance.
          </p>
        </div>

        <form on:submit|preventDefault={handleConfigureInstance} class="space-y-4">
          <div>
            <label for="instance-name" class="block text-sm font-medium mb-1.5">Nom de l'instance</label>
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
            <label for="instance-desc" class="block text-sm font-medium mb-1.5">Description</label>
            <textarea
              id="instance-desc"
              bind:value={instanceDescription}
              rows="3"
              class="w-full px-3 py-2.5 bg-[hsl(var(--secondary))] border border-[hsl(var(--border))] rounded-md text-sm focus:outline-none focus:ring-2 focus:ring-[hsl(var(--primary))] resize-none"
              placeholder="Une instance de streaming musical fédérée..."
            ></textarea>
          </div>

          <div class="flex gap-3">
            <button
              type="button"
              on:click={() => { currentStep = 1; error = ''; }}
              class="flex-1 py-2.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-md font-medium hover:opacity-90 transition"
            >
              Retour
            </button>
            <button
              type="submit"
              disabled={submitting}
              class="flex-1 py-2.5 bg-[hsl(var(--primary))] text-white rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
            >
              {#if submitting}
                Enregistrement...
              {:else}
                Suivant
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
          <h2 class="text-xl font-semibold mb-1">Réseau P2P & inscriptions</h2>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            Configurez comment votre instance interagit avec le réseau pair-à-pair.
          </p>
        </div>

        <div class="space-y-5">
          <!-- P2P toggle -->
          <div class="flex items-center justify-between p-4 bg-[hsl(var(--secondary))] rounded-lg border border-[hsl(var(--border))]">
            <div>
              <p class="text-sm font-medium">Réseau P2P (iroh)</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                Permet de partager et découvrir de la musique avec d'autres pairs.
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
              <p class="text-sm font-medium">Inscriptions ouvertes</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                Permet à n'importe qui de créer un compte sur votre instance.
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
                <p class="text-sm font-medium">Taille maximale d'upload</p>
                <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">
                  Taille maximale autorisée pour un fichier audio.
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
            on:click={() => { currentStep = 2; error = ''; }}
            class="flex-1 py-2.5 bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] rounded-md font-medium hover:opacity-90 transition"
          >
            Retour
          </button>
          <button
            type="button"
            on:click={handleComplete}
            disabled={submitting}
            class="flex-1 py-2.5 bg-[hsl(var(--primary))] text-white rounded-md font-medium hover:opacity-90 transition disabled:opacity-50"
          >
            {#if submitting}
              Finalisation...
            {:else}
              Suivant
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
          <h2 class="text-2xl font-bold">Votre instance est prête !</h2>
          <p class="text-[hsl(var(--muted-foreground))] mt-2">
            La configuration est terminée. Vous allez être redirigé vers l'accueil.
          </p>
        </div>

        <div class="bg-[hsl(var(--secondary))] rounded-lg p-5 text-left space-y-3 border border-[hsl(var(--border))]">
          <h3 class="text-sm font-semibold text-[hsl(var(--muted-foreground))] uppercase tracking-wide">Récapitulatif</h3>
          <div class="grid grid-cols-2 gap-y-2 text-sm">
            <span class="text-[hsl(var(--muted-foreground))]">Admin</span>
            <span class="font-medium">{adminUsername}</span>

            <span class="text-[hsl(var(--muted-foreground))]">Instance</span>
            <span class="font-medium">{instanceName}</span>

            <span class="text-[hsl(var(--muted-foreground))]">P2P</span>
            <span class="font-medium">{p2pEnabled ? "Activé" : "Désactivé"}</span>

            <span class="text-[hsl(var(--muted-foreground))]">Inscriptions</span>
            <span class="font-medium">{openRegistrations ? "Ouvertes" : "Fermées"}</span>

            <span class="text-[hsl(var(--muted-foreground))]">Upload max</span>
            <span class="font-medium">{maxUploadSizeMb} Mo</span>
          </div>
        </div>

        <div class="animate-spin w-6 h-6 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full mx-auto"></div>
        <p class="text-xs text-[hsl(var(--muted-foreground))]">Redirection en cours...</p>
      </div>
    {/if}
  </div>
{/if}
