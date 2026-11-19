<script lang="ts">
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const player = getPlayerStore();
  const queue = getQueueStore();
  const percent = $derived(Math.round(player.volume * 100));
</script>

<section class="space-y-4 border-b border-border py-6">
  <h2 class="text-lg font-semibold">{t("settings.audio")}</h2>
  <label class="flex flex-wrap items-center justify-between gap-3 text-sm">
    {t("player.volume")}
    <span class="flex items-center gap-3">
      <input type="range" min="0" max="100" value={percent} aria-label={t("player.volume")}
        aria-valuetext={percent + "%"} class="w-28 accent-primary sm:w-40"
        oninput={event => player.setVolume(event.currentTarget.valueAsNumber / 100)} />
      <span aria-hidden="true" class="w-10 text-right tabular-nums">{percent}%</span>
    </span>
  </label>
  <label class="flex items-center justify-between gap-3 text-sm">
    {t("player.autoplay")}
    <input type="checkbox" role="switch" checked={queue.autoplay} onchange={queue.toggleAutoplay} class="size-5 accent-primary" />
  </label>
  <p class="text-xs text-muted-foreground">{t("settings.supportedFormats")}</p>
</section>
