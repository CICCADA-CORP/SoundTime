<script lang="ts">
  import { ExternalLink, Play, RotateCw, Square } from "lucide-svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import PlayerRange from "$lib/components/player/PlayerRange.svelte";
  import StationIcon from "./StationIcon.svelte";
  import { live } from "./live.svelte";
  const player = getPlayerStore();
</script>

{#if live.station}
  {@const station = live.station}
  <section class="live-bar" aria-label={t("player.nowPlaying")}>
    <div class="size-10 shrink-0 sm:size-12"><StationIcon source={station.icon_url} /></div>
    <div class="min-w-0 flex-1">
      <a href="/webradios" class="block truncate text-sm font-semibold hover:underline">{station.name}</a>
      <p class="text-xs text-muted-foreground" role="status">
        {t(live.phase === "unsupported" ? "webradios.unsupported" : live.phase === "failed" ? "webradios.playError"
             : live.phase === "playing" ? "webradios.live" : live.phase === "ready" ? "shell.play" : "common.loading")}
        {#if live.url && (live.phase === "unsupported" || live.phase === "failed")}
          <a href={live.url} target="_blank" rel="noreferrer" class="inline-flex items-center gap-1 underline">
            {t("webradios.external")}<ExternalLink size={11} />
          </a>
        {/if}
      </p>
    </div>
    {#if live.phase === "ready"}
      <button class="grid size-10 place-items-center rounded-full border border-border" type="button"
        aria-label={t("webradios.playStation", { name: station.name })} onclick={live.resume}><Play size={16} /></button>
    {:else if live.phase === "failed"}
      <button class="grid size-10 place-items-center rounded-full border border-border" type="button"
        aria-label={t("radio.tryAgain")} onclick={() => player.playStation(station)}><RotateCw size={16} /></button>
    {/if}
    <button type="button" class="grid size-10 shrink-0 place-items-center rounded-full bg-foreground text-background"
      aria-label={t("webradios.stopStation", { name: station.name })} onclick={live.stop}><Square size={15} fill="currentColor" /></button>
    <div class="hidden w-24 sm:block">
      <PlayerRange label={t("player.volume")} max={1} step={0.01} value={player.volume} onchange={player.setVolume} />
    </div>
  </section>
{/if}

<style>
  .live-bar { position: fixed; z-index: 46; inset: auto 0 0 var(--workspace-left, var(--rail-width));
    min-height: 75px; display: flex; align-items: center; gap: 12px; padding: 8px clamp(16px, 3vw, 40px);
    border-top: 1px solid hsl(var(--border)); background: hsl(var(--background)); }
  @media(max-width: 767px) { .live-bar { inset: auto 0 var(--mobile-nav); min-height: 61px; gap: 8px; padding: 6px 12px; } }
</style>
