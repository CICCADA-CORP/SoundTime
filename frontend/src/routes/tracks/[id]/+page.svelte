<script lang="ts">
  import { page } from "$app/state";
  import { Flag, Pause, Pencil, Play, Trash2 } from "lucide-svelte";
  import { goto } from "$app/navigation";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import ReleaseHeader from "$lib/components/catalog/ReleaseHeader.svelte";
  import { availabilityLabel, canPlay } from "$lib/catalog/availability";
  import { api } from "$lib/api";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { Button } from "$lib/components/ui/button";
  import CatalogArtwork from "$lib/components/catalog/CatalogArtwork.svelte";
  import PlayerSaveActions from "$lib/components/player/PlayerSaveActions.svelte";
  import TrackActions from "$lib/components/catalog/TrackActions.svelte";
  import type { Track, TrackCredits } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { formatDuration } from "$lib/utils";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  const queue = getQueueStore();
  const player = getPlayerStore();
  const track = catalogQuery(() => api.get<Track>(`/tracks/${page.params.id}`));
  const result = catalogQuery(() => api.get<TrackCredits>(`/tracks/${page.params.id}/credits`));
  const credits = $derived(result.data);
  let editing = $state(false);
  let deleting = $state(false);
  let editForm = $state({ title: "", genre: "", year: "" as string, track_number: "" as string, disc_number: "" as string });
  let error: string | null = $state(null);
  let success: string | null = $state(null);
  let showReportForm = $state(false);

  let isOwner = $derived(credits?.uploaded_by && auth.user?.id === credits.uploaded_by);

  $effect(() => {
    void page.params.id;
    editing = false;
    deleting = false;
    showReportForm = false;
    error = success = null;
  });

  function toggleEdit() {
    if (!credits) return;
    editForm = {
      title: credits.title, genre: credits.genre ?? "", year: credits.year?.toString() ?? "",
      track_number: credits.track_number?.toString() ?? "", disc_number: credits.disc_number?.toString() ?? "",
    };
    editing = !editing;
    success = null;
  }

  async function saveEdit() {
    if (!credits) return;
    error = null;
    try {
      const body: Record<string, unknown> = {};
      if (editForm.title !== credits.title) body.title = editForm.title;
      if (editForm.genre !== (credits.genre ?? "")) body.genre = editForm.genre || null;
      if (editForm.year !== (credits.year?.toString() ?? "")) body.year = editForm.year ? parseInt(editForm.year) : null;
      if (editForm.track_number !== (credits.track_number?.toString() ?? "")) body.track_number = editForm.track_number ? parseInt(editForm.track_number) : null;
      if (editForm.disc_number !== (credits.disc_number?.toString() ?? "")) body.disc_number = editForm.disc_number ? parseInt(editForm.disc_number) : null;

      await api.put(`/tracks/${credits.id}`, body);
      success = t('track.metadataUpdated');
      track.retry();
      editing = false;
      result.retry();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function deleteTrack() {
    if (!credits || !confirm(t('track.deleteConfirm'))) return;
    deleting = true;
    try {
      await api.delete(`/tracks/${credits.id}`);
      await goto("/explore");
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      deleting = false;
    }
  }

  function play(item: Track) {
    if (player.currentTrack?.id === item.id) player.togglePlay();
    else queue.playQueue([item], 0, "collection");
  }
</script>

<svelte:head><title>{credits?.title ?? t("track.trackLabel")} — SoundTime</title></svelte:head>

{#if credits}
  <div class="max-w-4xl space-y-8">
    <ReleaseHeader kind="album" title={credits.title} image={credits.album_cover_url} label={t("track.trackLabel")}>
      {#snippet info()}
        <a href="/artists/{credits.artist_id}" class="hover:underline">{credits.artist_name}</a>
        {#if credits.album_title && credits.album_id}
          · <a href="/albums/{credits.album_id}" class="hover:underline">{credits.album_title}</a>
        {/if}
        · {credits.play_count} {credits.play_count === 1 ? t("track.play") : t("track.plays")}
      {/snippet}
      {#snippet actions()}
        {#if track.data}
          {@const item = track.data}
          {@const playing = player.currentTrack?.id === item.id && player.isPlaying}
          <Button disabled={!canPlay(item)} onclick={() => play(item)}>
            {#if playing}<Pause size={16} fill="currentColor" />{t("shell.pause")}
            {:else}<Play size={16} fill="currentColor" />{t("track.play")}{/if}
          </Button>
          {#if !canPlay(item)}<span class="text-sm text-muted-foreground">{availabilityLabel(item)}</span>{/if}
          {#if auth.isAuthenticated}<PlayerSaveActions track={item} />{/if}
        {:else if track.failed}
          <Button variant="outline" onclick={track.retry}>{t("radio.tryAgain")}</Button>
        {/if}
        {#if isOwner}
          <Button variant="outline" onclick={toggleEdit}><Pencil size={15} />{t(editing ? "track.cancelEdit" : "track.editMetadata")}</Button>
          <Button variant="ghost" class="text-destructive" onclick={deleteTrack} disabled={deleting}><Trash2 size={15} />{t(deleting ? "track.deleting" : "track.deleteTrack")}</Button>
        {:else if auth.isAuthenticated && track.data}
          <Button variant="ghost" onclick={() => showReportForm = true}><Flag size={15} />{t("track.reportTitle")}</Button>
        {/if}
      {/snippet}
    </ReleaseHeader>

    {#if showReportForm && track.data}<TrackActions track={track.data} action="report" close={() => showReportForm = false} />{/if}
    {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
    {#if success}<p role="status" class="text-sm">{success}</p>{/if}

    <!-- Edit Form -->
    {#if editing && isOwner}
      <form class="space-y-4 rounded-xl border bg-card p-6" onsubmit={(e) => { e.preventDefault(); saveEdit(); }}>
        <div>
          <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-title">{t('track.title')}</label>
          <input id="edit-title" type="text" bind:value={editForm.title} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
        </div>
        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-genre">{t('track.genre')}</label>
            <input id="edit-genre" type="text" bind:value={editForm.genre} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-year">{t('track.year')}</label>
            <input id="edit-year" type="number" bind:value={editForm.year} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
        </div>
        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-track">{t('track.trackNumber')}</label>
            <input id="edit-track" type="number" bind:value={editForm.track_number} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-disc">{t('track.discNumber')}</label>
            <input id="edit-disc" type="number" bind:value={editForm.disc_number} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
        </div>
        <Button type="submit">{t("common.save")}</Button>
      </form>
    {/if}

    <!-- Metadata Table -->
    <div class="overflow-hidden rounded-xl border bg-card">
      <h2 class="text-lg font-semibold p-5 border-b border-[hsl(var(--border))]">{t('track.technicalInfo')}</h2>
      <div class="divide-y divide-[hsl(var(--border))]">
        <div class="grid grid-cols-2 p-4">
          <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.duration')}</span>
          <span class="text-sm">{formatDuration(credits.duration_secs)}</span>
        </div>
        <div class="grid grid-cols-2 p-4">
          <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.format')}</span>
          <span class="text-sm uppercase">{credits.format}</span>
        </div>
        {#if credits.bitrate}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.bitrate')}</span>
            <span class="text-sm">{credits.bitrate} kbps</span>
          </div>
        {/if}
        {#if credits.sample_rate}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.sampleRate')}</span>
            <span class="text-sm">{(credits.sample_rate / 1000).toFixed(1)} kHz</span>
          </div>
        {/if}
        {#if credits.genre}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.genre')}</span>
            <span class="text-sm">{credits.genre}</span>
          </div>
        {/if}
        {#if credits.year}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.year')}</span>
            <span class="text-sm">{credits.year}</span>
          </div>
        {/if}
        {#if credits.track_number}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.trackLabel')}</span>
            <span class="text-sm">{credits.track_number}{credits.disc_number ? ` (${t('track.disc', { n: credits.disc_number })})` : ""}</span>
          </div>
        {/if}
        {#if credits.musicbrainz_id}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">MusicBrainz ID</span>
            <a href="https://musicbrainz.org/recording/{credits.musicbrainz_id}" target="_blank" rel="noopener" class="text-sm text-[hsl(var(--primary))] hover:underline font-mono">{credits.musicbrainz_id.slice(0,8)}…</a>
          </div>
        {/if}
        <div class="grid grid-cols-2 p-4">
          <span class="text-sm text-[hsl(var(--muted-foreground))]">{t('track.addedOn')}</span>
          <span class="text-sm">{new Date(credits.created_at).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" })}</span>
        </div>
      </div>
    </div>

    <!-- Artist Info -->
    <div class="overflow-hidden rounded-xl border bg-card">
      <h2 class="text-lg font-semibold p-5 border-b border-[hsl(var(--border))]">{t('track.artistSection')}</h2>
      <div class="p-5 flex items-center gap-4">
        <div class="size-16 shrink-0"><CatalogArtwork kind="artist" source={credits.artist_image} title={credits.artist_name} /></div>
        <div>
          <a href="/artists/{credits.artist_id}" class="font-medium hover:text-[hsl(var(--primary))] transition">{credits.artist_name}</a>
          {#if credits.artist_bio}
            <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1 line-clamp-2">{credits.artist_bio}</p>
          {/if}
          {#if credits.artist_musicbrainz_id}
            <a href="https://musicbrainz.org/artist/{credits.artist_musicbrainz_id}" target="_blank" rel="noopener" class="text-xs text-[hsl(var(--primary))] hover:underline mt-1 inline-block">MusicBrainz ↗</a>
          {/if}
        </div>
      </div>
    </div>

    <!-- Album Info -->
    {#if credits.album_id}
      <div class="overflow-hidden rounded-xl border bg-card">
        <h2 class="text-lg font-semibold p-5 border-b border-[hsl(var(--border))]">{t('track.albumSection')}</h2>
        <div class="p-5 flex items-center gap-4">
          <div class="size-16 shrink-0"><CatalogArtwork kind="album" source={credits.album_cover_url} title={credits.album_title ?? ""} /></div>
          <div>
            <a href="/albums/{credits.album_id}" class="font-medium hover:text-[hsl(var(--primary))] transition">{credits.album_title}</a>
            {#if credits.album_genre}
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{credits.album_genre}</p>
            {/if}
            {#if credits.album_year}
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{credits.album_year}</p>
            {/if}
            {#if credits.album_musicbrainz_id}
              <a href="https://musicbrainz.org/release/{credits.album_musicbrainz_id}" target="_blank" rel="noopener" class="text-xs text-[hsl(var(--primary))] hover:underline mt-1 inline-block">MusicBrainz ↗</a>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  </div>
{:else}
  <CatalogState loading={result.loading} failed={result.failed} status={result.status} retry={result.retry} message={t("track.notFound")} />
{/if}
