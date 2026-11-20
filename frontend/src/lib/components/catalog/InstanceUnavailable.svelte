<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  import { onDestroy } from "svelte";
  import { showcaseAudio, showcaseTracks } from "$lib/showcase";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import SoundTimeSignature from "../SoundTimeSignature.svelte";
  import SessionTracks from "../home/SessionTracks.svelte";
  import AudioPlayer from "../AudioPlayer.svelte";

  let { retry }: { retry: () => void } = $props();
  let demo = $state(false);
  const player = getPlayerStore();
  onDestroy(() => {
    if (demo && player.currentTrack && showcaseAudio(player.currentTrack.id)) player.pause();
  });
</script>

<svelte:head><title>{t("access.unavailable")} — SoundTime</title></svelte:head>
<main id="main-content" class="offline">
  <SoundTimeSignature width={132} />
  <h1 class="mt-8 text-2xl font-semibold">{t("access.unavailable")}</h1>
  <p class="mt-2 text-sm text-muted-foreground">{t("access.retryHint")}</p>
  <div class="my-6 flex flex-wrap gap-2">
    <Button onclick={retry}>{t("access.retry")}</Button>
    {#if !demo}<Button variant="outline" onclick={() => demo = true}>{t("access.openDemo")}</Button>{/if}
  </div>
  {#if demo}
    <section aria-label={t("access.demo")}>
      <h2 class="mb-3 text-lg font-medium">{t("access.demo")} · After Hours</h2>
      <SessionTracks tracks={showcaseTracks} />
    </section>
    <AudioPlayer />
  {/if}
</main>

<style>
  .offline { --workspace-left: 0px; --mobile-nav: env(safe-area-inset-bottom, 0px); max-width: 38rem; min-height: 100svh; margin-inline: auto; padding: clamp(3rem, 14vh, 8rem) 1.5rem 7rem; }
</style>
