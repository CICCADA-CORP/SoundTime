<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { AudioLines, LoaderCircle, Play, Radio, Square } from "lucide-svelte";
  import { homeApi } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";
  import { Button } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";

  const auth = getAuthStore();
  const radio = getRadioStore();
  const queue = getQueueStore();
  const radioTrack = $derived(!radio.loading && (queue.sourceContext === "radio" || queue.sourceContext === "autoplay") ? queue.currentTrack : null);
  const genres = catalogQuery(homeApi.genres);
</script>

<svelte:head><title>{t("nav.radio")} — SoundTime</title></svelte:head>

<div class="radio-page space-y-7 sm:space-y-9">
  <CatalogHeader title={t("nav.radio")} />

  <section class="mix glass-surface">
    <div class="mix-copy">
      <h2>{t("radio.personalMix")}</h2>
      <p>{t("radio.mixSubtitle")}</p>
    </div>
    {#if auth.isAuthenticated}
      <Button class="rounded-full px-5" disabled={radio.loading} onclick={() => radio.startRadio("personal_mix", { label: t("radio.personalMix") })}>
        {#if radio.loading && radio.seedType === "personal_mix"}<LoaderCircle class="motion-safe:animate-spin" />
        {:else}<Play size={15} fill="currentColor" />{/if}
        {t("radio.startGenreRadio")}
      </Button>
    {:else}
      <Button class="rounded-full px-5" href={signInLink(page.url)}>{t("nav.signIn")}</Button>
    {/if}
  </section>

  {#if radio.active}
    <section class="flex flex-wrap items-center gap-3 rounded-xl bg-muted/65 px-4 py-3">
      {#if radioTrack}
        <span class="size-10 shrink-0 rounded-lg" aria-hidden="true">
          <CatalogArtwork kind="album" title={radioTrack.title} source={radioTrack.cover_url} artworkId={radioTrack.album_id ?? radioTrack.id} />
        </span>
      {:else}
        <span class="grid size-9 place-items-center text-primary"><AudioLines size={21} /></span>
      {/if}
      <div class="min-w-0 flex-1" role="status">
        <p class="text-xs text-muted-foreground">{radio.loading ? t("common.loading") : t("radio.activeRadio")}</p>
        <h2 class="truncate font-semibold">{radio.seedLabel}</h2>
      </div>
      <Button variant="outline" size="sm" onclick={radio.stopRadio}><Square size={13} />{t("radio.stop")}</Button>
    </section>
  {/if}
  {#if radio.error}
    <div class="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-muted/65 p-4" role="alert">
      <p class="text-sm">{t("radio.error")}</p>
      <Button variant="outline" size="sm" onclick={radio.retry}>{t("radio.tryAgain")}</Button>
    </div>
  {:else if radio.exhausted}
    <p role="status" class="text-sm text-muted-foreground">{t("radio.exhausted")}</p>
  {/if}

  <section class="space-y-4">
    <SectionHeading title={t("radio.byGenre")} />
    {#if genres.data?.length}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,13rem),1fr))] gap-x-4 gap-y-1">
        {#each genres.data as genre (genre)}
          <Button variant="outline" href={auth.isAuthenticated ? undefined : "/login"} disabled={radio.loading}
            class="group h-14 w-full justify-start gap-3 rounded-none border-x-0 border-t-0 border-b border-border px-1 text-left shadow-none hover:bg-muted"
            aria-label={(auth.isAuthenticated ? t("radio.startGenreRadio") : t("auth.signIn")) + " · " + genre}
            onclick={() => auth.isAuthenticated && radio.startRadio("genre", { genre, label: genre })}>
            <span class="grid size-7 shrink-0 place-items-center text-primary"><Radio size={17} /></span>
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
  .radio-page { container-type: inline-size; }
  .mix { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 1rem 1.5rem; padding: clamp(1.15rem, 3cqi, 1.9rem); border-radius: 1.2rem; }
  .mix-copy { flex: 1 1 15rem; }
  .mix h2 { font-size: clamp(1.3rem, 3cqi, 1.85rem); font-weight: 640; line-height: 1.12; letter-spacing: -.035em; }
  .mix p { margin-top: .4rem; max-width: 48ch; color: hsl(var(--muted-foreground)); font-size: .9rem; }
</style>
