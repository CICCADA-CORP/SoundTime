<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "$lib/api";
  import { invalidatePlaylists } from "$lib/catalog/playlists.svelte";
  import type { Playlist } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";

  let { playlist, onsave, oncancel }: {
    playlist?: Playlist; onsave: (saved: Playlist) => void; oncancel: () => void;
  } = $props();
  const initial = untrack(() => playlist);
  let name = $state(initial?.name ?? "");
  let description = $state(initial?.description ?? "");
  let cover = $state(initial?.cover_url ?? "");
  let isPublic = $state(initial?.is_public ?? true);
  let busy = $state(false);
  let error = $state("");

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || busy) return;
    busy = true;
    error = "";
    try {
      const fields = { name: name.trim(), description: description.trim(), is_public: isPublic };
      const saved = playlist
        ? await api.put<Playlist>(`/playlists/${playlist.id}`, { ...fields, cover_url: cover.trim() })
        : await api.post<Playlist>("/playlists", fields);
      invalidatePlaylists();
      onsave(saved);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("common.error");
    } finally {
      busy = false;
    }
  }
</script>

<form class="space-y-4" onsubmit={submit}>
  <label class="block space-y-1.5"><span class="text-sm font-medium">{t("playlists.nameLabel")}</span>
    <Input name="name" bind:value={name} required placeholder={t("playlists.newName")} /></label>
  <label class="block space-y-1.5"><span class="text-sm font-medium">{t("playlists.descriptionLabel")}</span>
    <textarea class="block w-full rounded-md border border-input bg-background px-3 py-2 text-sm" name="description"
      rows="3" bind:value={description} placeholder={t("playlists.descriptionPlaceholder")}></textarea></label>
  {#if playlist}
    <label class="block space-y-1.5"><span class="text-sm font-medium">{t("playlists.coverUrl")}</span>
      <Input type="url" name="cover" bind:value={cover} /></label>
  {/if}
  <label class="flex items-center gap-2 text-sm">
    <input type="checkbox" bind:checked={isPublic} class="size-4 accent-primary" />{t("playlists.publicPlaylist")}
  </label>
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
  <div class="flex justify-end gap-2">
    <Button variant="ghost" type="button" onclick={oncancel}>{t("common.cancel")}</Button>
    <Button type="submit" disabled={busy || !name.trim()}>{busy ? t("common.loading") : playlist ? t("common.save") : t("playlists.create")}</Button>
  </div>
</form>
