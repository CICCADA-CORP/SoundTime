<script lang="ts">
  import SettingsSection from "./SettingsSection.svelte";
  import { Dialog } from "bits-ui";
  import { goto } from "$app/navigation";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  let open = $state(false);
  let password = $state("");
  let error = $state("");
  let busy = $state(false);
  const warning = $derived(t("settings.deleteModalWarning").split(/\{\/?\s*strong\s*\}/));

  function changeOpen(value: boolean) {
    if (busy) return;
    open = value;
    password = "";
    error = "";
  }

  async function remove(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    if (!password.trim()) {
      error = t("settings.passwordRequired");
      return;
    }
    error = "";
    busy = true;
    try {
      await auth.deleteAccount(password);
      open = false;
      await goto("/");
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("settings.deleteError");
    } finally {
      busy = false;
    }
  }
</script>

<SettingsSection title={t("settings.dangerZone")}>
  <p class="text-sm text-muted-foreground">{t("settings.deleteWarning")}</p>
  <div class="flex flex-wrap gap-2">
    <Button variant="outline" onclick={auth.logout}>{t("nav.logout")}</Button>
    <Dialog.Root {open} onOpenChange={changeOpen}>
      <Dialog.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline" class="text-destructive">{t("settings.deleteAccount")}</Button>
        {/snippet}
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay class="ui-overlay z-[150]" />
        <Dialog.Content class="ui-dialog z-[151]">
          <Dialog.Title class="text-lg font-semibold">{t("settings.deleteConfirm")}</Dialog.Title>
          <Dialog.Description class="my-3 text-sm text-muted-foreground">
            {warning[0]}<strong>{warning[1]}</strong>{warning[2]}
          </Dialog.Description>
          <form class="space-y-4" onsubmit={remove} aria-busy={busy}>
            <label class="grid gap-1 text-sm">{t("settings.confirmPassword")}
              <Input type="password" bind:value={password} autocomplete="current-password" />
            </label>
            {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
            <div class="flex flex-wrap justify-end gap-2">
              <Button variant="outline" disabled={busy} onclick={() => changeOpen(false)}>{t("common.cancel")}</Button>
              <Button type="submit" variant="destructive" disabled={busy}>{t(busy ? "common.loading" : "settings.deleteButton")}</Button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  </div>
</SettingsSection>
