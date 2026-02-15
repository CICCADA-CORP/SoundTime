<script lang="ts">
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { Copy, Check } from "lucide-svelte";
  import { goto } from "$app/navigation";
  import { t, getLocale, setLocale, supportedLocales, localeNames } from "$lib/i18n/index.svelte";
  import type { Locale } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();

  let audioQuality = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("soundtime_audio_quality") ?? "high"
      : "high"
  );
  let crossfade = $state(
    typeof window !== "undefined"
      ? parseInt(localStorage.getItem("soundtime_crossfade") ?? "0", 10)
      : 0
  );
  let copied = $state(false);
  let showDeleteConfirm = $state(false);
  let deletePassword = $state("");
  let deleteError = $state("");
  let deleteLoading = $state(false);

  // Email change
  let newEmail = $state("");
  let emailPassword = $state("");
  let emailError = $state("");
  let emailSuccess = $state("");
  let emailLoading = $state(false);

  // Password change
  let currentPassword = $state("");
  let newPassword = $state("");
  let confirmNewPassword = $state("");
  let passwordError = $state("");
  let passwordSuccess = $state("");
  let passwordLoading = $state(false);

  const deleteWarningParts = $derived(t('settings.deleteModalWarning').split(/\{\/?\s*strong\s*\}/));

  function saveQuality(val: string) {
    audioQuality = val;
    if (typeof window !== "undefined") localStorage.setItem("soundtime_audio_quality", val);
  }

  function saveCrossfade(val: number) {
    crossfade = val;
    if (typeof window !== "undefined") localStorage.setItem("soundtime_crossfade", val.toString());
  }

  function copyInstanceId() {
    if (auth.user?.instance_id) {
      navigator.clipboard.writeText(`@${auth.user.instance_id}`);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    }
  }
</script>

<svelte:head><title>{t('settings.title')} — SoundTime</title></svelte:head>

<div class="max-w-2xl mx-auto space-y-8">
  <h1 class="text-2xl font-bold">{t('settings.title')}</h1>

  {#if !auth.isAuthenticated}
    <p class="text-[hsl(var(--muted-foreground))] text-center py-10">{t('settings.loginRequired')}</p>
  {:else}
    <section class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4">
      <h2 class="text-lg font-semibold">{t('settings.account')}</h2>
      <div class="grid grid-cols-2 gap-4">
        <div>
          <span class="text-xs text-[hsl(var(--muted-foreground))] block mb-1">{t('auth.username')}</span>
          <p class="text-sm">{auth.user?.username}</p>
        </div>
        <div>
          <span class="text-xs text-[hsl(var(--muted-foreground))] block mb-1">{t('auth.email')}</span>
          <p class="text-sm">{auth.user?.email}</p>
        </div>
        <div>
          <span class="text-xs text-[hsl(var(--muted-foreground))] block mb-1">{t('settings.role')}</span>
          <p class="text-sm capitalize">{auth.user?.role}</p>
        </div>
        <div>
          <span class="text-xs text-[hsl(var(--muted-foreground))] block mb-1">{t('settings.p2pNodeId')}</span>
          <div class="flex items-center gap-2">
            <p class="text-sm font-mono text-[hsl(var(--primary))]">@{auth.user?.instance_id}</p>
            <button
              class="p-1 rounded hover:bg-[hsl(var(--secondary))] transition"
              onclick={copyInstanceId}
              title="Copier"
            >
              {#if copied}
                <Check size={14} class="text-green-400" />
              {:else}
                <Copy size={14} class="text-[hsl(var(--muted-foreground))]" />
              {/if}
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- Change Email -->
    <section class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4">
      <h2 class="text-lg font-semibold">{t('settings.changeEmail')}</h2>
      {#if emailSuccess}
        <p class="text-sm text-green-400">{emailSuccess}</p>
      {/if}
      {#if emailError}
        <p class="text-sm text-red-400">{emailError}</p>
      {/if}
      <form class="space-y-3" onsubmit={async (e) => {
        e.preventDefault();
        if (!newEmail.trim()) { emailError = t('settings.emailRequired'); return; }
        if (!emailPassword.trim()) { emailError = t('settings.passwordRequired'); return; }
        emailLoading = true;
        emailError = "";
        emailSuccess = "";
        try {
          await auth.updateEmail(newEmail.trim(), emailPassword);
          emailSuccess = t('settings.emailChanged');
          newEmail = "";
          emailPassword = "";
        } catch (err: unknown) {
          emailError = (err instanceof Error ? err.message : String(err)) ?? t('common.error');
        } finally { emailLoading = false; }
      }}>
        <div>
          <label for="new-email" class="text-sm block mb-1">{t('settings.newEmail')}</label>
          <input id="new-email" type="email" bind:value={newEmail} placeholder={t('settings.newEmailPlaceholder')} class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm" />
        </div>
        <div>
          <label for="email-password" class="text-sm block mb-1">{t('settings.confirmPassword')}</label>
          <input id="email-password" type="password" bind:value={emailPassword} placeholder={t('settings.passwordPlaceholder')} class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm" />
        </div>
        <button type="submit" class="px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium disabled:opacity-50" disabled={emailLoading}>
          {emailLoading ? t('common.loading') : t('settings.changeEmail')}
        </button>
      </form>
    </section>

    <!-- Change Password -->
    <section class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4">
      <h2 class="text-lg font-semibold">{t('settings.changePassword')}</h2>
      {#if passwordSuccess}
        <p class="text-sm text-green-400">{passwordSuccess}</p>
      {/if}
      {#if passwordError}
        <p class="text-sm text-red-400">{passwordError}</p>
      {/if}
      <form class="space-y-3" onsubmit={async (e) => {
        e.preventDefault();
        if (!currentPassword.trim()) { passwordError = t('settings.passwordRequired'); return; }
        if (newPassword.length < 8) { passwordError = t('settings.passwordMinLength'); return; }
        if (newPassword !== confirmNewPassword) { passwordError = t('settings.passwordMismatch'); return; }
        passwordLoading = true;
        passwordError = "";
        passwordSuccess = "";
        try {
          await auth.updatePassword(currentPassword, newPassword);
          passwordSuccess = t('settings.passwordChanged');
          currentPassword = "";
          newPassword = "";
          confirmNewPassword = "";
        } catch (err: unknown) {
          passwordError = (err instanceof Error ? err.message : String(err)) ?? t('common.error');
        } finally { passwordLoading = false; }
      }}>
        <div>
          <label for="current-password" class="text-sm block mb-1">{t('settings.currentPassword')}</label>
          <input id="current-password" type="password" bind:value={currentPassword} placeholder={t('settings.passwordPlaceholder')} class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm" />
        </div>
        <div>
          <label for="new-password" class="text-sm block mb-1">{t('settings.newPassword')}</label>
          <input id="new-password" type="password" bind:value={newPassword} placeholder={t('settings.newPasswordPlaceholder')} class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm" />
        </div>
        <div>
          <label for="confirm-new-password" class="text-sm block mb-1">{t('auth.confirmPassword')}</label>
          <input id="confirm-new-password" type="password" bind:value={confirmNewPassword} placeholder={t('settings.confirmNewPasswordPlaceholder')} class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm" />
        </div>
        <button type="submit" class="px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium disabled:opacity-50" disabled={passwordLoading}>
          {passwordLoading ? t('common.loading') : t('settings.changePassword')}
        </button>
      </form>
    </section>

    <section class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4">
      <h2 class="text-lg font-semibold">{t('settings.audio')}</h2>
      <div class="space-y-4">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm font-medium">{t('settings.audioQuality')}</p>
            <p class="text-xs text-[hsl(var(--muted-foreground))]">
              {#if audioQuality === "master"}
                {t('settings.qualityMaster')}
              {:else if audioQuality === "very_high"}
                {t('settings.qualityVeryHigh')}
              {:else if audioQuality === "high"}
                {t('settings.qualityHigh')}
              {:else if audioQuality === "medium"}
                {t('settings.qualityMedium')}
              {:else}
                {t('settings.qualityLow')}
              {/if}
            </p>
          </div>
          <select
            class="bg-[hsl(var(--secondary))] text-sm rounded-lg px-3 py-1.5 border border-[hsl(var(--border))]"
            value={audioQuality}
            onchange={(e) => saveQuality((e.target as HTMLSelectElement).value)}
          >
            <option value="master">{t('settings.qualityMasterLabel')}</option>
            <option value="very_high">{t('settings.qualityVeryHighLabel')}</option>
            <option value="high">{t('settings.qualityHighLabel')}</option>
            <option value="medium">{t('settings.qualityMediumLabel')}</option>
            <option value="low">{t('settings.qualityLowLabel')}</option>
          </select>
        </div>
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm font-medium">Crossfade</p>
            <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('settings.crossfadeBetween', { value: crossfade.toString() })}</p>
          </div>
          <input
            type="range"
            min="0"
            max="12"
            value={crossfade}
            class="w-32 accent-[hsl(var(--primary))]"
            oninput={(e) => saveCrossfade(parseInt((e.target as HTMLInputElement).value, 10))}
          />
        </div>
      </div>
      <p class="text-xs text-[hsl(var(--muted-foreground))] mt-2">
        {t('settings.supportedFormats')}
      </p>
    </section>

    <!-- Language Selector -->
    <section class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4">
      <h2 class="text-lg font-semibold">{t('settings.language')}</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm font-medium">{t('settings.languageSelect')}</p>
          <p class="text-xs text-[hsl(var(--muted-foreground))]">{localeNames[getLocale()]}</p>
        </div>
        <select
          class="bg-[hsl(var(--secondary))] text-sm rounded-lg px-3 py-1.5 border border-[hsl(var(--border))]"
          value={getLocale()}
          onchange={(e) => setLocale((e.target as HTMLSelectElement).value as Locale)}
        >
          {#each supportedLocales as loc}
            <option value={loc}>{localeNames[loc]}</option>
          {/each}
        </select>
      </div>
    </section>

    <section class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4">
      <h2 class="text-lg font-semibold text-red-400">{t('settings.dangerZone')}</h2>
      <div class="flex gap-3 flex-wrap">
        <button
          class="px-4 py-2 bg-red-500/20 text-red-400 rounded-lg text-sm hover:bg-red-500/30 transition"
          onclick={auth.logout}
        >
          {t('nav.logout')}
        </button>
        <button
          class="px-4 py-2 bg-red-600/20 text-red-400 rounded-lg text-sm hover:bg-red-600/30 transition border border-red-500/30"
          onclick={() => { showDeleteConfirm = true; deleteError = ""; deletePassword = ""; }}
        >
        {t('settings.deleteAccount')}
        </button>
      </div>
      <p class="text-xs text-[hsl(var(--muted-foreground))]">
        {t('settings.deleteWarning')}
      </p>
    </section>

    <!-- Delete Account Modal -->
    {#if showDeleteConfirm}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="fixed inset-0 bg-black/60 z-50 flex items-center justify-center p-4" onclick={() => showDeleteConfirm = false} onkeydown={(e) => e.key === 'Escape' && (showDeleteConfirm = false)} role="dialog" tabindex="-1">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="bg-[hsl(var(--card))] rounded-xl p-6 w-full max-w-md shadow-2xl space-y-4" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
          <h3 class="text-lg font-semibold text-red-400">{t('settings.deleteConfirm')}</h3>
          <p class="text-sm text-[hsl(var(--muted-foreground))]">
            {deleteWarningParts[0]}<strong class="text-red-400">{deleteWarningParts[1] ?? ''}</strong>{deleteWarningParts[2] ?? ''}
          </p>
          {#if deleteError}
            <p class="text-sm text-red-400">{deleteError}</p>
          {/if}
          <form onsubmit={async (e) => {
            e.preventDefault();
            if (!deletePassword.trim()) { deleteError = t('settings.passwordRequired'); return; }
            deleteLoading = true;
            deleteError = "";
            try {
              await auth.deleteAccount(deletePassword);
              showDeleteConfirm = false;
              goto("/");
            } catch (err: unknown) {
              deleteError = (err instanceof Error ? err.message : String(err)) ?? t('settings.deleteError');
            } finally {
              deleteLoading = false;
            }
          }}>
            <label class="block text-sm mb-1" for="delete-password">{t('settings.confirmPassword')}</label>
            <input
              id="delete-password"
              type="password"
              bind:value={deletePassword}
              placeholder={t('settings.passwordPlaceholder')}
              class="w-full bg-[hsl(var(--secondary))] rounded px-3 py-2 text-sm mb-3"
            />
            <div class="flex gap-2 justify-end">
              <button
                type="button"
                class="px-4 py-2 text-sm rounded hover:bg-[hsl(var(--secondary))] transition"
                onclick={() => showDeleteConfirm = false}
              >
                {t('common.cancel')}
              </button>
              <button
                type="submit"
                class="px-4 py-2 bg-red-600 text-white text-sm rounded hover:bg-red-700 transition disabled:opacity-50"
                disabled={deleteLoading}
              >
                {deleteLoading ? t('common.loading') : t('settings.deleteButton')}
              </button>
            </div>
          </form>
        </div>
      </div>
    {/if}
  {/if}
</div>
