<script lang="ts">
  import { Monitor, Moon, Sun } from "lucide-svelte";
  import { onMount } from "svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { dark = $bindable(false) }: { dark?: boolean } = $props();
  const options = [
    { value: "light", label: "authAppearance.light", icon: Sun },
    { value: "dark", label: "authAppearance.dark", icon: Moon },
    { value: "system", label: "authAppearance.system", icon: Monitor },
  ] as const;
  type Mode = (typeof options)[number]["value"];
  const key = "soundtime_auth_appearance";
  const name = $props.id();
  let mode = $state<Mode>("system");
  let preferredScheme: MediaQueryList | undefined;

  function apply(value: string | null) {
    mode = value === "light" || value === "dark" ? value : "system";
    dark = mode === "dark" || (mode === "system" && !!preferredScheme?.matches);
  }

  function choose(value: Mode) {
    apply(value);
    try {
      localStorage.setItem(key, value);
    } catch {
      // The selection still works for this visit when browser storage is unavailable.
    }
  }

  onMount(() => {
    preferredScheme = window.matchMedia?.("(prefers-color-scheme: dark)");
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(key);
    } catch {
      // Use the system setting when browser storage is unavailable.
    }
    apply(saved);

    const updateSystem = () => apply(mode);
    function updateStored(event: StorageEvent) {
      if (event.key === key || event.key === null) apply(event.newValue);
    }
    preferredScheme?.addEventListener("change", updateSystem);
    window.addEventListener("storage", updateStored);
    return () => {
      preferredScheme?.removeEventListener("change", updateSystem);
      window.removeEventListener("storage", updateStored);
    };
  });
</script>

<fieldset class="appearance">
  <legend class="sr-only">{t("authAppearance.label")}</legend>
  {#each options as option (option.value)}
    <label title={t(option.label)}>
      <input type="radio" {name} value={option.value} checked={mode === option.value}
        aria-label={t(option.label)} onchange={() => choose(option.value)} />
      <span><option.icon size={16} strokeWidth={1.65} aria-hidden="true" /></span>
    </label>
  {/each}
</fieldset>

<style>
  .appearance { display: inline-flex; width: max-content; padding: 3px; border: 1px solid hsl(var(--border)); border-radius: 10px; background: hsl(var(--sidebar)); }
  label { position: relative; display: grid; width: 29px; height: 27px; color: hsl(var(--muted-foreground)); }
  input { position: absolute; inset: 0; z-index: 1; margin: 0; width: 100%; height: 100%; opacity: 0; cursor: pointer; }
  span { display: grid; place-items: center; border-radius: 6px; }
  input:checked + span { background: hsl(var(--background)); color: hsl(var(--foreground)); box-shadow: 0 1px 3px rgb(0 0 0 / .12); }
  input:focus-visible + span { outline: 2px solid hsl(var(--foreground)); outline-offset: 1px; }
  label:hover { color: hsl(var(--foreground)); }
</style>
