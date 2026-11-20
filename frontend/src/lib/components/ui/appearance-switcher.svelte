<script lang="ts">
  import { Monitor, Moon, Sun } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { appearance } from "$lib/stores/appearance.svelte";

  const options = [
    { value: "light", label: "authAppearance.light", icon: Sun },
    { value: "dark", label: "authAppearance.dark", icon: Moon },
    { value: "system", label: "authAppearance.system", icon: Monitor },
  ] as const;
  const name = $props.id();
</script>

<fieldset class="appearance">
  <legend class="sr-only">{t("authAppearance.label")}</legend>
  {#each options as option (option.value)}
    <label title={t(option.label)}>
      <input type="radio" {name} value={option.value} checked={appearance.mode === option.value}
        aria-label={t(option.label)} onchange={() => appearance.choose(option.value)} />
      <span><option.icon size={16} strokeWidth={1.65} aria-hidden="true" /></span>
    </label>
  {/each}
</fieldset>

<style>
  .appearance { display: inline-flex; width: max-content; padding: 3px; border-radius: var(--radius-control); background: hsl(var(--secondary)); }
  label { position: relative; display: grid; width: 31px; height: 28px; color: hsl(var(--muted-foreground)); }
  input { position: absolute; inset: 0; z-index: 1; margin: 0; width: 100%; height: 100%; opacity: 0; cursor: pointer; }
  span { display: grid; place-items: center; border-radius: calc(var(--radius-control) - 3px); }
  input:checked + span { background: hsl(var(--card)); color: hsl(var(--foreground)); box-shadow: 0 1px 4px rgb(16 21 36 / .14); }
  input:focus-visible + span { outline: 2px solid hsl(var(--ring)); outline-offset: 1px; }
  label:hover { color: hsl(var(--foreground)); }
</style>
