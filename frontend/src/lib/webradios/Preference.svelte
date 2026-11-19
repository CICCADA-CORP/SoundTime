<script lang="ts">
  import { untrack } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { webPreferences } from "./preferences.svelte";

  const auth = getAuthStore();
  const member = $derived(auth.isAuthenticated ? auth.user?.id ?? null : null);
  let saving = $state(false);
  let error = $state(false);
  $effect(() => {
    const id = member;
    untrack(() => { void webPreferences.refresh(id); });
  });
  async function change(event: Event & { currentTarget: HTMLInputElement }) {
    const value = event.currentTarget.checked;
    event.currentTarget.checked = !value;
    saving = true;
    error = false;
    try { await webPreferences.setEnabled(value, member); }
    catch { error = true; }
    finally { saving = false; }
  }
</script>

{#if webPreferences.status}
  <div class="space-y-2">
    <label class="flex items-center justify-between gap-4 text-sm">
      <span>{t("webradios.personalSetting")}</span>
      <input type="checkbox" role="switch" class="size-5 accent-primary" checked={webPreferences.userEnabled}
        disabled={saving || !webPreferences.status.instance_enabled} onchange={change} />
    </label>
    {#if !webPreferences.status.instance_enabled}<p class="text-xs text-muted-foreground">{t("webradios.instanceOff")}</p>{/if}
    {#if error}<p role="alert" class="text-sm text-destructive">{t("webradios.saveError")}</p>{/if}
  </div>
{:else if webPreferences.failed}
  <button type="button" class="text-sm underline" onclick={() => webPreferences.refresh(member)}>{t("radio.tryAgain")}</button>
{/if}
