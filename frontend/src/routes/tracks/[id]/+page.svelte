<script lang="ts">
  import { onDestroy } from "svelte";
  import { page } from "$app/state";
  import { Flag, Pause, Pencil, Play, Trash2 } from "lucide-svelte";
  import { goto } from "$app/navigation";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CachePin from "$lib/components/catalog/CachePin.svelte";
  import SectionHeading from "$lib/components/catalog/SectionHeading.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import ReleaseHeader from "$lib/components/catalog/ReleaseHeader.svelte";
  import { availabilityLabel, canPlay } from "$lib/catalog/availability";
  import { api, apiFetch } from "$lib/api";
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
  const track = catalogQuery(signal => apiFetch<Track>(`/tracks/${page.params.id}`, { signal }));
  const item = $derived(track.data);
  const result = catalogQuery(signal => apiFetch<TrackCredits>(`/tracks/${page.params.id}/credits`, { signal }));
  const credits = $derived(result.data);
  let editing = $state(false);
  let deleting = $state(false);
  let editForm = $state({ title: "", genre: "", year: "" as string, track_number: "" as string, disc_number: "" as string });
  let error: string | null = $state(null);
  let success: string | null = $state(null);
  let showReportForm = $state(false);

  const isOwner = $derived(item?.uploaded_by && auth.user?.id === item.uploaded_by);

  let mounted = true;
  onDestroy(() => { mounted = false; });

  function toggleEdit() {
    if (!item) return;
    editForm = {
      title: item.title, genre: item.genre ?? "", year: item.year?.toString() ?? "",
      track_number: item.track_number?.toString() ?? "", disc_number: item.disc_number?.toString() ?? "",
    };
    editing = !editing;
    success = null;
  }

  async function saveEdit() {
    if (!item) return;
    error = null;
    try {
      const body: Record<string, unknown> = {};
      if (editForm.title !== item.title) body.title = editForm.title;
      if (editForm.genre !== (item.genre ?? "")) body.genre = editForm.genre || null;
      if (editForm.year !== (item.year?.toString() ?? "")) body.year = editForm.year ? parseInt(editForm.year) : null;
      if (editForm.track_number !== (item.track_number?.toString() ?? "")) body.track_number = editForm.track_number ? parseInt(editForm.track_number) : null;
      if (editForm.disc_number !== (item.disc_number?.toString() ?? "")) body.disc_number = editForm.disc_number ? parseInt(editForm.disc_number) : null;

      await api.put(`/tracks/${item.id}`, body);
      if (!mounted) return;
      success = t('track.metadataUpdated');
      track.retry();
      editing = false;
      result.retry();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function deleteTrack() {
    if (!item || !confirm(t('track.deleteConfirm'))) return;
    deleting = true;
    try {
      await api.delete(`/tracks/${item.id}`);
      if (mounted) await goto("/explore");
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

<svelte:head><title>{item?.title ?? t("track.trackLabel")} — SoundTime</title></svelte:head>

{#if item}
  <div class="max-w-4xl space-y-8">
    <ReleaseHeader artworkId={item.album_id ?? item.id} kind="album" title={item.title} image={item.cover_url} label={t("track.trackLabel")}>
      {#snippet info()}
        {#if item.artist_name}<a href="/artists/{item.artist_id}" class="hover:underline">{item.artist_name}</a> · {/if}
        {#if item.album_title && item.album_id}<a href="/albums/{item.album_id}" class="hover:underline">{item.album_title}</a> · {/if}
        {item.play_count} {item.play_count === 1 ? t("track.play") : t("track.plays")}
      {/snippet}
      {#snippet actions()}
        {@const playing = player.currentTrack?.id === item.id && player.isPlaying}
        <Button disabled={!canPlay(item)} onclick={() => play(item)}>
          {#if playing}<Pause size={16} fill="currentColor" />{t("shell.pause")}
          {:else}<Play size={16} fill="currentColor" />{t("track.play")}{/if}
        </Button>
        {#if !canPlay(item)}<span class="text-sm text-muted-foreground">{availabilityLabel(item)}</span>{/if}
        {#if auth.isAuthenticated}<PlayerSaveActions track={item} />{/if}
        <CachePin scope="tracks" id={item.id} />
        {#if isOwner}
          <Button variant="outline" onclick={toggleEdit}><Pencil size={15} />{t(editing ? "track.cancelEdit" : "track.editMetadata")}</Button>
          <Button variant="ghost" class="text-destructive" disabled={deleting} onclick={deleteTrack}><Trash2 size={15} />{t(deleting ? "track.deleting" : "track.deleteTrack")}</Button>
        {:else if auth.isAuthenticated}
          <Button variant="ghost" onclick={() => showReportForm = true}><Flag size={15} />{t("track.reportTitle")}</Button>
        {/if}
      {/snippet}
    </ReleaseHeader>

    {#if showReportForm}<TrackActions track={item} action="report" close={() => showReportForm = false} />{/if}
    {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
    {#if success}<p role="status" class="text-sm">{success}</p>{/if}

    <!-- Edit Form -->
    {#if editing && isOwner}
      <form class="glass-surface space-y-4 rounded-xl p-5" onsubmit={(e) => { e.preventDefault(); saveEdit(); }}>
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
    <div class="min-w-0">
      <SectionHeading title={t('track.technicalInfo')} />
      <dl class="metadata">
        <div>
          <dt>{t('track.duration')}</dt>
          <dd>{formatDuration(item.duration_secs)}</dd>
        </div>
        <div>
          <dt>{t('track.format')}</dt>
          <dd class="uppercase">{item.format}</dd>
        </div>
        {#if item.bitrate}
          <div>
            <dt>{t('track.bitrate')}</dt>
            <dd>{item.bitrate} kbps</dd>
          </div>
        {/if}
        {#if item.sample_rate}
          <div>
            <dt>{t('track.sampleRate')}</dt>
            <dd>{(item.sample_rate / 1000).toFixed(1)} kHz</dd>
          </div>
        {/if}
        {#if item.genre}
          <div>
            <dt>{t('track.genre')}</dt>
            <dd>{item.genre}</dd>
          </div>
        {/if}
        {#if item.year}
          <div>
            <dt>{t('track.year')}</dt>
            <dd>{item.year}</dd>
          </div>
        {/if}
        {#if item.track_number}
          <div>
            <dt>{t('track.trackLabel')}</dt>
            <dd>{item.track_number}{item.disc_number ? ` (${t('track.disc', { n: item.disc_number })})` : ""}</dd>
          </div>
        {/if}
        {#if item.musicbrainz_id}
          <div>
            <dt>MusicBrainz ID</dt>
            <dd><a href="https://musicbrainz.org/recording/{item.musicbrainz_id}" target="_blank" rel="noopener" class="text-sm text-[hsl(var(--primary))] hover:underline">{item.musicbrainz_id.slice(0,8)}…</a></dd>
          </div>
        {/if}
        <div>
          <dt>{t('track.addedOn')}</dt>
          <dd>{new Date(item.created_at).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" })}</dd>
        </div>
      </dl>
    </div>

    {#if result.loading || result.failed}
      <CatalogState loading={result.loading} failed={result.failed} status={result.status} retry={result.retry} />
    {/if}
    {#if credits?.artist_name}
      <div class="min-w-0">
        <SectionHeading title={t('track.artistSection')} />
        <div class="flex min-w-0 items-center gap-4 px-1 py-2">
          <div class="size-16 shrink-0 rounded-full"><CatalogArtwork artworkId={credits.artist_id} kind="artist" source={credits.artist_image} title={credits.artist_name} /></div>
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
    {/if}

    <!-- Album Info -->
    {#if credits?.album_id}
      <div class="min-w-0">
        <SectionHeading title={t('track.albumSection')} />
        <div class="flex min-w-0 items-center gap-4 px-1 py-2">
          <div class="size-16 shrink-0 rounded-lg"><CatalogArtwork artworkId={credits.album_id} kind="album" source={credits.album_cover_url} title={credits.album_title ?? ""} /></div>
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
  <CatalogState layout="detail" loading={track.loading} failed={track.failed} status={track.status} retry={track.retry} message={t("track.notFound")} />
{/if}

<style>
  .metadata > div { display: grid; grid-template-columns: minmax(7rem, .8fr) minmax(0, 1.2fr); gap: .75rem; border-bottom: 1px solid hsl(var(--border)); padding: .6rem 0; }
  dt { color: hsl(var(--muted-foreground)); font-size: .85rem; }
  dd { overflow-wrap: anywhere; font-size: .86rem; line-height: 1.5; font-variant-numeric: tabular-nums; }
</style>
