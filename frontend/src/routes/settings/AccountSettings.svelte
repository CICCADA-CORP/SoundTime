<script lang="ts">
  import { Copy, Check } from "lucide-svelte";
  import { onDestroy } from "svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  const auth = getAuthStore();
  let copied = $state(false);
  let clearCopied: ReturnType<typeof setTimeout>;
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

  async function copyInstanceId() {
    if (!auth.user?.instance_id) return;
    try {
      await navigator.clipboard.writeText("@" + auth.user.instance_id);
      copied = true;
      clearTimeout(clearCopied);
      clearCopied = setTimeout(() => copied = false, 2000);
    } catch { copied = false; }
  }

  onDestroy(() => clearTimeout(clearCopied));
</script>

<section class="space-y-4 border-b border-border py-6">
  <h2 class="text-lg font-semibold">{t('settings.account')}</h2>
  <div class="grid gap-4 sm:grid-cols-2">
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
          title={t(copied ? "common.copied" : "settings.copyId")}
        >
          {#if copied}
            <Check size={14} class="text-green-700" />
          {:else}
            <Copy size={14} class="text-[hsl(var(--muted-foreground))]" />
          {/if}
        </button>
      </div>
    </div>
  </div>
</section>

<!-- Change Email -->
<section class="space-y-4 border-b border-border py-6">
  <h2 class="text-lg font-semibold">{t('settings.changeEmail')}</h2>
  {#if emailSuccess}
    <p role="status" class="text-sm text-foreground">{emailSuccess}</p>
  {/if}
  {#if emailError}
    <p role="alert" class="text-sm text-destructive">{emailError}</p>
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
      <Input id="new-email" type="email" autocomplete="email" bind:value={newEmail} placeholder={t('settings.newEmailPlaceholder')} class="max-w-md shadow-none" />
    </div>
    <div>
      <label for="email-password" class="text-sm block mb-1">{t('settings.confirmPassword')}</label>
      <Input id="email-password" type="password" autocomplete="current-password" bind:value={emailPassword} placeholder={t('settings.passwordPlaceholder')} class="max-w-md shadow-none" />
    </div>
    <Button type="submit" disabled={emailLoading}>
      {emailLoading ? t('common.loading') : t('settings.changeEmail')}
    </Button>
  </form>
</section>

<!-- Change Password -->
<section class="space-y-4 border-b border-border py-6">
  <h2 class="text-lg font-semibold">{t('settings.changePassword')}</h2>
  {#if passwordSuccess}
    <p role="status" class="text-sm text-foreground">{passwordSuccess}</p>
  {/if}
  {#if passwordError}
    <p role="alert" class="text-sm text-destructive">{passwordError}</p>
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
      <Input id="current-password" type="password" autocomplete="current-password" bind:value={currentPassword} placeholder={t('settings.passwordPlaceholder')} class="max-w-md shadow-none" />
    </div>
    <div>
      <label for="new-password" class="text-sm block mb-1">{t('settings.newPassword')}</label>
      <Input id="new-password" type="password" autocomplete="new-password" bind:value={newPassword} placeholder={t('settings.newPasswordPlaceholder')} class="max-w-md shadow-none" />
    </div>
    <div>
      <label for="confirm-new-password" class="text-sm block mb-1">{t('auth.confirmPassword')}</label>
      <Input id="confirm-new-password" type="password" autocomplete="new-password" bind:value={confirmNewPassword} placeholder={t('settings.confirmNewPasswordPlaceholder')} class="max-w-md shadow-none" />
    </div>
    <Button type="submit" disabled={passwordLoading}>
      {passwordLoading ? t('common.loading') : t('settings.changePassword')}
    </Button>
  </form>
</section>
