<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { AudioLines, LoaderCircle, Play, Radio, Square } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import { Button } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";

  const auth = getAuthStore();
  const radio = getRadioStore();
  const genres = catalogQuery(homeApi.genres);
</script>

<svelte:head><title>{t("nav.radio")} — SoundTime</title></svelte:head>

<div class="space-y-7 sm:space-y-9">
  <CatalogHeader title={t("nav.radio")} />

  <section class="mix">
    <img src="/art/slow-bloom.webp" alt="" class="artwork" />
    <div class="min-w-0 space-y-2.5">
      <h2 class="text-2xl font-semibold leading-tight tracking-tight sm:text-4xl">{t("radio.personalMix")}</h2>
      <p class="max-w-sm text-sm text-muted-foreground">{t("radio.mixSubtitle")}</p>
      <div class="pt-1">
        {#if auth.isAuthenticated}
          <Button disabled={radio.loading} onclick={() => radio.startRadio("personal_mix", { label: t("radio.personalMix") })}>
            {#if radio.loading && radio.seedType === "personal_mix"}<LoaderCircle class="motion-safe:animate-spin" />
            {:else}<Play size={15} fill="currentColor" />{/if}
            {t("radio.startGenreRadio")}
          </Button>
        {:else}
          <Button href={signInLink(page.url)}>{t("nav.signIn")}</Button>
        {/if}
      </div>
    </div>
  </section>

  {#if radio.active}
    <section class="flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card p-4">
      <span class="grid size-10 place-items-center rounded-full bg-accent text-primary"><AudioLines size={21} /></span>
      <div class="min-w-0 flex-1" role="status">
        <p class="text-xs text-muted-foreground">{radio.loading ? t("common.loading") : t("radio.activeRadio")}</p>
        <h2 class="truncate font-semibold">{radio.seedLabel}</h2>
      </div>
      <Button variant="outline" size="sm" onclick={radio.stopRadio}><Square size={13} />{t("radio.stop")}</Button>
    </section>
  {/if}
  {#if radio.error}
    <div class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border p-4" role="alert">
      <p class="text-sm">{t("radio.error")}</p>
      <Button variant="outline" size="sm" onclick={radio.retry}>{t("radio.tryAgain")}</Button>
    </div>
  {:else if radio.exhausted}
    <p role="status" class="text-sm text-muted-foreground">{t("radio.exhausted")}</p>
  {/if}

  <section class="space-y-4">
    <h2 class="text-xl font-semibold tracking-tight">{t("radio.byGenre")}</h2>
    {#if genres.data?.length}
      <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2 xl:grid-cols-3">
        {#each genres.data as genre (genre)}
          <Button variant="outline" href={auth.isAuthenticated ? undefined : "/login"} disabled={radio.loading}
            class="group h-14 w-full justify-start gap-3 rounded-xl border-border px-3 text-left hover:bg-muted"
            aria-label={(auth.isAuthenticated ? t("radio.startGenreRadio") : t("auth.signIn")) + " · " + genre}
            onclick={() => auth.isAuthenticated && radio.startRadio("genre", { genre, label: genre })}>
            <span class="grid size-8 shrink-0 place-items-center rounded-md bg-accent text-primary"><Radio size={17} /></span>
            <span class="truncate">{genre}</span>
            {#if radio.loading && radio.seedLabel === genre}<LoaderCircle class="ml-auto motion-safe:animate-spin" />
            {:else}<Play class="ml-auto text-muted-foreground group-hover:text-primary" size={14} />{/if}
          </Button>
        {/each}
      </div>
    {:else}
      <CatalogState loading={genres.loading} failed={genres.failed} status={genres.status} retry={genres.retry} message={t("common.noResults")} />
    {/if}
  </section>
</div>

<style>
  .mix {
    display: flex; align-items: center; gap: clamp(1rem, 3.5vw, 2.75rem);
    padding: clamp(1rem, 2.5vw, 2rem); border-radius: 1rem;
    background: hsl(var(--accent) / .56);
  }
  .artwork {
    width: clamp(5rem, 15vw, 10.5rem); aspect-ratio: 1; flex: none;
    border-radius: .7rem; object-fit: cover;
  }
  @media (max-width: 420px) {
    .mix { gap: .85rem; }
    .artwork { width: 4.5rem; }
  }
</style>
