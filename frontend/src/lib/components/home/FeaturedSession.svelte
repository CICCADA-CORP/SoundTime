<script lang="ts">
  import { ListMusic, Play } from "lucide-svelte";
  import { Button } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { accountLink } from "$lib/access/navigation";
  import type { PlaybackSource, Track } from "$lib/types";
  import { homeCopy } from "./copy";
  import SessionTracks from "./SessionTracks.svelte";

  let { title, tracks, source, empty, editorial }: {
    title: string; tracks: Track[]; source: PlaybackSource; empty: boolean; editorial: boolean;
  } = $props();
  let showTracks = $state(false);
  const queue = getQueueStore();
  const auth = getAuthStore();
  const copy = $derived(homeCopy());
  const trackCount = $derived(tracks.length === 1 ? t("workspaceHome.oneTrack") : t("workspaceHome.trackCount", {count: tracks.length}));
</script>

<section aria-label={title}>
  <header>
    <div class="min-w-0">
      <h1>{copy.title}</h1>
      {#if tracks.length}
        <p>{title}<span aria-hidden="true"> · </span>{trackCount}</p>
      {:else if empty}
        <p>{copy.empty}</p>
      {/if}
    </div>
    {#if tracks.length}
      <div class="actions">
        <Button variant="outline" class="h-8 rounded-lg px-3.5 text-[13px] shadow-none" onclick={() => queue.playQueue(tracks, 0, source)}>
          <Play size={14} />{copy.play}
        </Button>
        {#if editorial}
          <Button variant="ghost" size="icon" class="h-8 w-8" aria-label={copy.tracklist} title={copy.tracklist}
            aria-expanded={showTracks} aria-controls="feature-tracks" onclick={() => showTracks = !showTracks}><ListMusic size={17} /></Button>
        {/if}
      </div>
    {:else if empty}
      <Button href={auth.isAuthenticated ? "/upload" : accountLink("/login", "/upload")} variant="outline">
        {t(auth.isAuthenticated ? "home.uploadFirst" : "shell.signInUpload")}
      </Button>
    {/if}
  </header>
  {#if showTracks && editorial && tracks.length}
    <div id="feature-tracks" class="mt-4"><SessionTracks {tracks} {source} /></div>
  {/if}
</section>

<style>
  header { display: flex; align-items: end; justify-content: space-between; flex-wrap: wrap; gap: 12px; }
  h1 { font-size: clamp(1.5rem, 1.75vw, 1.8rem); letter-spacing: -.04em; font-weight: 600; line-height: 1.25; }
  p { margin-top: 5px; color: hsl(var(--muted-foreground)); font-size: .91rem; line-height: 1.5; }
  .actions { display: flex; align-items: center; gap: 2px; }
</style>
