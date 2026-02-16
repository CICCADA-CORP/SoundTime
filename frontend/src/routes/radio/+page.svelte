<script lang="ts">
  import { getRadioStore } from "$lib/stores/radio.svelte";
  import { homeApi } from "$lib/api";
  import { t } from "$lib/i18n/index.svelte";
  import { Radio, Play, Square, Music, AlertCircle, Sparkles, Loader2, Info } from "lucide-svelte";

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

<div class="space-y-8 max-w-5xl p-6">

  <!-- ─── 1. Hero Section ──────────────────────────────────────────── -->
  <section class="relative overflow-hidden rounded-2xl bg-gradient-to-br from-[hsl(var(--primary))] via-purple-600 to-pink-500 p-8 md:p-12">
    <!-- Decorative background circles -->
    <div class="absolute -top-10 -right-10 w-40 h-40 bg-white/10 rounded-full blur-2xl"></div>
    <div class="absolute -bottom-8 -left-8 w-32 h-32 bg-white/5 rounded-full blur-2xl"></div>
    <div class="absolute top-1/2 left-1/3 w-24 h-24 bg-white/5 rounded-full blur-3xl"></div>

    <div class="relative flex items-center gap-5">
      <div class="w-16 h-16 md:w-20 md:h-20 rounded-2xl bg-white/20 backdrop-blur-sm flex items-center justify-center flex-shrink-0">
        <Radio class="w-8 h-8 md:w-10 md:h-10 text-white" />
      </div>
      <div>
        <h1 class="text-3xl md:text-4xl font-bold text-white">{t('radio.heroTitle')}</h1>
        <p class="text-base md:text-lg text-white/70 mt-1">{t('radio.heroSubtitle')}</p>
      </div>
    </div>
  </section>

  <!-- ─── 2. Active Radio Status Card ──────────────────────────────── -->
  {#if radio.active}
    <section class="rounded-xl border border-[hsl(var(--border))] bg-[hsl(var(--card))] p-6 shadow-sm">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div class="flex items-center gap-4">
          <div class="relative flex-shrink-0">
            <div class="w-10 h-10 rounded-full bg-green-500/20 flex items-center justify-center">
              <div class="w-3 h-3 rounded-full bg-green-500 animate-pulse"></div>
            </div>
          </div>
          <div>
            <h2 class="text-lg font-semibold">{t('radio.activeRadio')}</h2>
            <p class="text-sm text-[hsl(var(--muted-foreground))]">
              {t('radio.basedOn')}
              <span class="font-medium text-[hsl(var(--foreground))]">{radio.seedLabel}</span>
              · {radio.playedCount} {t('radio.tracksPlayed')}
            </p>
            <p class="text-xs text-[hsl(var(--primary))] mt-1">
              {t('radio.nowPlayingFrom')}: {radio.seedLabel}
            </p>
          </div>
        </div>
        <button
          class="px-5 py-2.5 rounded-lg bg-[hsl(var(--destructive))] text-[hsl(var(--destructive-foreground))] hover:opacity-90 transition-all text-sm font-medium flex items-center gap-2 self-start sm:self-center"
          onclick={() => radio.stopRadio()}
        >
          <Square class="w-4 h-4" />
          {t('radio.stop')}
        </button>
      </div>
    </section>
  {/if}

  <!-- ─── 3. Error Display ─────────────────────────────────────────── -->
  {#if radio.error}
    <section class="rounded-xl border border-[hsl(var(--destructive))]/30 bg-[hsl(var(--destructive))]/10 p-5">
      <div class="flex items-start gap-3">
        <AlertCircle class="w-5 h-5 text-[hsl(var(--destructive))] flex-shrink-0 mt-0.5" />
        <div class="flex-1">
          <p class="text-sm font-medium text-[hsl(var(--destructive))]">{t('radio.error')}</p>
          <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">{radio.error}</p>
        </div>
        <button
          class="px-4 py-1.5 rounded-lg bg-[hsl(var(--destructive))] text-[hsl(var(--destructive-foreground))] hover:opacity-90 transition text-xs font-medium flex-shrink-0"
          onclick={() => radio.clearError()}
        >
          {t('radio.tryAgain')}
        </button>
      </div>
    </section>
  {/if}

  <!-- ─── 4. Personal Mix Section ──────────────────────────────────── -->
  <section>
    <h2 class="text-xl font-bold mb-4">{t('radio.personalMix')}</h2>
    <button
      class="w-full relative overflow-hidden rounded-2xl bg-gradient-to-r from-[hsl(var(--primary))] via-purple-600 to-pink-500 p-6 md:p-8 text-left group transition-all hover:shadow-xl hover:shadow-[hsl(var(--primary)/0.2)] active:scale-[0.99]"
      onclick={() => radio.startRadio("personal_mix", { label: t('radio.personalMix') })}
      disabled={radio.loading}
    >
      <!-- Background decoration -->
      <div class="absolute -top-10 -right-10 w-40 h-40 bg-white/10 rounded-full blur-2xl group-hover:scale-150 transition-transform duration-500"></div>
      <div class="absolute -bottom-8 -left-8 w-32 h-32 bg-white/5 rounded-full blur-2xl"></div>

      <div class="relative flex items-center gap-4">
        <div class="w-14 h-14 rounded-full bg-white/20 backdrop-blur-sm flex items-center justify-center flex-shrink-0 group-hover:scale-110 transition-transform">
          {#if radio.loading}
            <Loader2 class="w-7 h-7 text-white animate-spin" />
          {:else}
            <Play class="w-7 h-7 text-white ml-0.5" fill="currentColor" />
          {/if}
        </div>
        <div class="flex-1 min-w-0">
          <h3 class="text-xl md:text-2xl font-bold text-white">{t('radio.playMyMix')}</h3>
          <p class="text-sm text-white/70 mt-0.5">{t('radio.mixSubtitle')}</p>
        </div>
        <div class="hidden md:flex items-center justify-center w-10 h-10 rounded-full bg-white/20 group-hover:bg-white/30 transition flex-shrink-0">
          <Sparkles class="w-5 h-5 text-white" />
        </div>
      </div>
    </button>
    <p class="text-xs text-[hsl(var(--muted-foreground))] mt-3 flex items-center gap-1.5">
      <Sparkles class="w-3.5 h-3.5" />
      {t('radio.mixImproveHint')}
    </p>
  </section>

  <!-- ─── 5. Genre Grid Section ────────────────────────────────────── -->
  <section>
    <h2 class="text-xl font-bold mb-4">{t('radio.byGenre')}</h2>
    {#if loadingGenres}
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
        {#each Array(10) as _}
          <div class="h-14 rounded-xl bg-[hsl(var(--secondary))] animate-pulse"></div>
        {/each}
      </div>
    {:else if genres.length === 0}
      <div class="rounded-xl border border-[hsl(var(--border))] bg-[hsl(var(--card))] p-8 text-center">
        <Music class="w-8 h-8 text-[hsl(var(--muted-foreground))] mx-auto mb-2" />
        <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('radio.stationEmpty')}</p>
      </div>
    {:else}
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
        {#each genres as genre}
          <button
            class="group relative overflow-hidden rounded-xl border border-[hsl(var(--border))] bg-[hsl(var(--card))] p-4 text-left transition-all hover:scale-[1.03] hover:shadow-md hover:border-[hsl(var(--primary))]/40 hover:bg-[hsl(var(--primary))]/5 active:scale-[0.98]"
            onclick={() => radio.startRadio("genre", { genre, label: genre })}
            disabled={radio.loading}
          >
            <div class="flex items-center gap-3">
              <div class="w-8 h-8 rounded-lg bg-[hsl(var(--secondary))] flex items-center justify-center flex-shrink-0 group-hover:bg-[hsl(var(--primary))]/20 transition-colors">
                <Music class="w-4 h-4 text-[hsl(var(--muted-foreground))] group-hover:text-[hsl(var(--primary))] transition-colors" />
              </div>
              <span class="text-sm font-medium truncate">{genre}</span>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  </section>

  <!-- ─── 6. Exhausted State ───────────────────────────────────────── -->
  {#if radio.exhausted}
    <section class="rounded-xl border border-[hsl(var(--border))] bg-[hsl(var(--secondary))] p-5">
      <div class="flex items-center gap-3">
        <Info class="w-5 h-5 text-[hsl(var(--muted-foreground))] flex-shrink-0" />
        <p class="text-sm text-[hsl(var(--muted-foreground))]">{t('radio.stationEmpty')}</p>
      </div>
    </section>
  {/if}
</div>
