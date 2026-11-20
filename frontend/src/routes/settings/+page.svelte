<script lang="ts">
  import SettingsSection from "./SettingsSection.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import { Button } from "$lib/components/ui/button";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t, getLocale, setLocale, supportedLocales, localeNames, type Locale } from "$lib/i18n/index.svelte";
  import { accountLink } from "$lib/access/navigation";
  import Preference from "$lib/webradios/Preference.svelte";
  import AccountSettings from "./AccountSettings.svelte";
  import AccountSession from "./AccountSession.svelte";
  import PlaybackSettings from "./PlaybackSettings.svelte";
  import LastfmSettings from "./LastfmSettings.svelte";

  const auth = getAuthStore();
</script>

<svelte:head><title>{t("settings.title")} — SoundTime</title></svelte:head>

<CatalogHeader title={t("settings.title")} />
{#if !auth.isAuthenticated}
  <CatalogState message={t("settings.loginRequired")}>
    <Button href={accountLink("/login", "/settings")}>{t("auth.signIn")}</Button>
  </CatalogState>
{/if}
<div class="settings glass-surface">
  {#if auth.isAuthenticated}
    <AccountSettings />
    <PlaybackSettings />
  {/if}
  <SettingsSection title={t("nav.webradios")}>
    <div id="webradios"><Preference /></div>
  </SettingsSection>
  {#if auth.isAuthenticated}
    <SettingsSection title={t("settings.language")}>
      <label class="flex flex-wrap items-center justify-between gap-3 text-sm">
        {t("settings.languageSelect")}
        <select class="rounded-md border border-input bg-background px-3 py-2"
          value={getLocale()} onchange={event => setLocale(event.currentTarget.value as Locale)}>
          {#each supportedLocales as locale (locale)}
            <option value={locale}>{localeNames[locale]}</option>
          {/each}
        </select>
      </label>
    </SettingsSection>
    <LastfmSettings />
    <AccountSession />
  {/if}
</div>

<style>
  .settings { max-width: 58rem; container: settings / inline-size; border-radius: 1.2rem; padding-inline: clamp(1rem, 3vw, 2rem); margin-top: 1.25rem; }
</style>
