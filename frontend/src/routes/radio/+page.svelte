<script lang="ts">
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { homeApi } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { Radio, Play, Square, Music } from "lucide-svelte";

  const radio = getRadioStore();

  let genres = $state<string[]>([]);
  let loadingGenres = $state(true);

  async function loadGenres() {
    try {
      genres = await homeApi.genres();
    } catch {
      genres = [];
    } finally {
      loadingGenres = false;
    }
  }

  // Load genres on mount (this runs in browser only)
  if (typeof window !== "undefined") {
    loadGenres();
  }
</script>

<svelte:head>
  <title>{t('nav.radio')} — SoundTime</title>
</svelte:head>

<div class="p-6 space-y-8 max-w-5xl">
  <!-- Header -->
  <div class="flex items-center gap-3">
    <Radio class="w-8 h-8 text-[hsl(var(--primary))]" />
    <h1 class="text-3xl font-bold">{t('nav.radio')}</h1>
  </div>

  <!-- Active Radio Status -->
  {#if radio.active}
    <div class="rounded-xl border border-[hsl(var(--border))] bg-[hsl(var(--card))] p-6">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-4">
          <div class="w-3 h-3 rounded-full bg-[hsl(var(--primary))] animate-pulse"></div>
          <div>
            <h2 class="text-lg font-semibold">{t('radio.activeRadio')}</h2>
            <p class="text-sm text-[hsl(var(--muted-foreground))]">
              {t('radio.basedOn')} <span class="font-medium text-[hsl(var(--foreground))]">{radio.seedLabel}</span>
              · {radio.playedCount} {t('radio.tracksPlayed')}
            </p>
          </div>
        </div>
        <button
          class="px-4 py-2 rounded-lg bg-[hsl(var(--destructive))] text-[hsl(var(--destructive-foreground))] hover:opacity-90 transition text-sm font-medium flex items-center gap-2"
          onclick={() => radio.stopRadio()}
        >
          <Square class="w-4 h-4" />
          {t('radio.stop')}
        </button>
      </div>
    </div>
  {/if}

  <!-- My Mix Section -->
  <section>
    <h2 class="text-xl font-semibold mb-4">{t('radio.personalMix')}</h2>
    <button
      class="w-full sm:w-auto px-8 py-6 rounded-xl bg-gradient-to-r from-[hsl(var(--primary))] to-[hsl(var(--primary))]/70 text-[hsl(var(--primary-foreground))] hover:opacity-90 transition font-semibold text-lg flex items-center gap-3"
      onclick={() => radio.startRadio("personal_mix", { label: t('radio.personalMix') })}
      disabled={radio.loading}
    >
      <Play class="w-6 h-6" />
      {t('radio.playMyMix')}
    </button>
    <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">{t('radio.noHistory')}</p>
  </section>

  <!-- By Genre Section -->
  <section>
    <h2 class="text-xl font-semibold mb-4">{t('radio.byGenre')}</h2>
    {#if loadingGenres}
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
        {#each Array(10) as _}
          <div class="h-12 rounded-lg bg-[hsl(var(--secondary))] animate-pulse"></div>
        {/each}
      </div>
    {:else if genres.length === 0}
      <p class="text-[hsl(var(--muted-foreground))]">No genres available</p>
    {:else}
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
        {#each genres as genre}
          <button
            class="px-4 py-3 rounded-lg bg-[hsl(var(--secondary))] hover:bg-[hsl(var(--primary))]/20 transition text-sm font-medium flex items-center gap-2 text-left"
            onclick={() => radio.startRadio("genre", { genre, label: genre })}
            disabled={radio.loading}
          >
            <Music class="w-4 h-4 text-[hsl(var(--muted-foreground))] shrink-0" />
            {genre}
          </button>
        {/each}
      </div>
    {/if}
  </section>

  <!-- Exhausted message -->
  {#if radio.exhausted}
    <div class="rounded-lg border border-[hsl(var(--border))] bg-[hsl(var(--secondary))] p-4 text-sm text-[hsl(var(--muted-foreground))]">
      {t('radio.exhausted')}
    </div>
  {/if}
</div>
