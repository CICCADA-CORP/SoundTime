<script lang="ts" module>
  export type TrackDialog = "playlist" | "credits" | "share" | "report";
</script>

<script lang="ts">
  import { Dialog } from "bits-ui";
  import { ListMusic } from "lucide-svelte";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { ownedPlaylists, invalidatePlaylists } from "$lib/catalog/playlists.svelte";
  import PageControls from "./PageControls.svelte";
  import CatalogState from "./CatalogState.svelte";
  import { t, type TranslationKey } from "$lib/i18n/index.svelte";
  import type { Track, TrackCredits } from "$lib/types";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";

  let { action, track, close }: { action: TrackDialog; track: Track; close: () => void } = $props();
  const titles: Record<TrackDialog, TranslationKey> = {
    playlist: "track.addToPlaylist", credits: "track.viewCredits", share: "track.share", report: "track.reportTitle",
  };
  let page = $state(1);
  const playlists = ownedPlaylists(() => action === "playlist" ? page : null);
  const credits = catalogQuery(() => action === "credits" ? api.get<TrackCredits>(`/tracks/${track.id}/credits`) : null);
  let url = $derived(typeof window === "undefined" ? "" : new URL(`/tracks/${track.id}`, window.location.origin).href);
  let error = $state("");
  let busy = $state(false);
  let reason = $state("");
  let sent = $state(false);
  let copied = $state(false);

  async function submit(playlistId?: string) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      if (action === "playlist" && playlistId) {
        await api.post(`/playlists/${playlistId}/tracks`, { track_id: track.id });
        close();
        invalidatePlaylists();
      } else if (action === "share") {
        await navigator.clipboard.writeText(url);
        copied = true;
      } else if (action === "report" && reason.trim()) {
        await api.post(`/tracks/${track.id}/report`, { reason: reason.trim() });
        sent = true;
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("common.error");
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root open onOpenChange={open => { if (!open) close(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="ui-overlay z-[201]" />
    <Dialog.Content class="ui-dialog z-[202] space-y-4">
      <Dialog.Title class="text-lg font-semibold">{t(titles[action])}</Dialog.Title>
      <Dialog.Description class="truncate text-sm text-muted-foreground">{track.title}</Dialog.Description>
      {#if action === "playlist"}
        {#if playlists.loading}<p role="status">{t("common.loading")}</p>
        {:else if playlists.failed}<CatalogState failed status={playlists.status} retry={playlists.retry} />
        {:else if !playlists.data?.data.length}<p class="text-sm text-muted-foreground">{t("track.noPlaylists")}</p>
        {:else}
          <div class="max-h-60 space-y-1 overflow-y-auto">
            {#each playlists.data.data as item (item.id)}
              <Button variant="ghost" class="w-full justify-start" disabled={busy} onclick={() => submit(item.id)}><ListMusic size={16} />{item.name}</Button>
            {/each}
          </div>
          <PageControls {page} total={playlists.data.total_pages} select={(next) => page = next} />
        {/if}
      {:else if action === "credits"}
        {#if credits.loading}<p role="status">{t("common.loading")}</p>
        {:else if credits.data}
          {@const c = credits.data}
          <dl class="space-y-2 text-sm">
            {#each [[t("track.artist"), c.artist_name ?? c.artist], [t("track.album"), c.album_title ?? c.album], [t("track.genre"), c.genre], [t("track.year"), c.year], [t("track.format"), c.format], [t("track.bitrate"), c.bitrate && c.bitrate + " kbps"], [t("track.sampleRate"), c.sample_rate && c.sample_rate + " Hz"], [t("track.uploadedBy"), c.uploaded_by_username]] as [label, value]}
              {#if value}<div class="flex justify-between gap-4"><dt class="text-muted-foreground">{label}</dt><dd class="text-right">{value}</dd></div>{/if}
            {/each}
            {#if c.musicbrainz_id}
              <div><dt class="text-muted-foreground">MusicBrainz</dt><dd><a href="https://musicbrainz.org/recording/{c.musicbrainz_id}" target="_blank" rel="noreferrer" class="break-all text-primary">{c.musicbrainz_id}</a></dd></div>
            {/if}
          </dl>
        {:else}<p class="text-sm text-muted-foreground">{t("track.noCredits")}</p>{/if}
      {:else if action === "share"}
        <div class="flex gap-2">
          <Input type="url" value={url} readonly aria-label={t("track.share")} />
          <Button disabled={busy} onclick={() => submit()}>{copied ? t("common.copied") : t("common.copy")}</Button>
        </div>
      {:else if sent}
        <p role="status" class="text-sm">{t("track.reportSent")}</p>
      {:else}
        <textarea class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm" rows="4" bind:value={reason}
          maxlength={500} aria-label={t("track.reportPlaceholder")} placeholder={t("track.reportPlaceholder")}></textarea>
      {/if}
      {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
      <div class="flex justify-end gap-2">
        <Button variant="secondary" onclick={close}>{t(action === "playlist" || (action === "report" && !sent) ? "common.cancel" : "common.close")}</Button>
        {#if action === "report" && !sent}
          <Button disabled={busy || !reason.trim()} onclick={() => submit()}>{t("track.sendReport")}</Button>
        {/if}
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
