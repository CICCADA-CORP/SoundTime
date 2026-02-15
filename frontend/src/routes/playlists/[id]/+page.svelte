<script lang="ts">
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Playlist } from "$lib/types";
  import TrackList from "$lib/components/TrackList.svelte";
  import { getQueueStore } from "$lib/stores/queue.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const queue = getQueueStore();
  const auth = getAuthStore();
  let playlist = $state<Playlist | null>(null);
  let loading = $state(true);
  let editing = $state(false);
  let deleting = $state(false);
  let editForm = $state({ name: "", description: "", is_public: true, cover_url: "" });
  let error: string | null = $state(null);
  let success: string | null = $state(null);

  let isOwner = $derived(playlist !== null && auth.user != null && playlist.owner_id === auth.user.id);

  onMount(async () => {
    const id = $page.params.id;
    try {
      const data = await api.get<Playlist>(`/playlists/${id}`);
      playlist = {
        id: data.id,
        name: data.name,
        description: data.description,
        is_public: data.is_public,
        owner_id: data.user_id ?? data.owner_id,
        user_id: data.user_id,
        cover_url: data.cover_url,
        created_at: data.created_at,
        updated_at: data.updated_at,
        tracks: data.tracks ?? [],
      };
      if (playlist) {
        editForm = {
          name: playlist.name,
          description: playlist.description ?? "",
          is_public: playlist.is_public,
          cover_url: playlist.cover_url ?? "",
        };
      }
    } catch (e) { console.error('Failed to load playlist:', e); } finally { loading = false; }
  });

  function playAll() {
    if (playlist?.tracks) {
      queue.playQueue(playlist.tracks);
    }
  }

  async function saveEdit() {
    if (!playlist) return;
    error = null;
    try {
      const body: Record<string, unknown> = {};
      if (editForm.name !== playlist.name) body.name = editForm.name;
      if (editForm.description !== (playlist.description ?? "")) body.description = editForm.description;
      if (editForm.is_public !== playlist.is_public) body.is_public = editForm.is_public;
      if (editForm.cover_url !== (playlist.cover_url ?? "")) body.cover_url = editForm.cover_url || null;

      const updated = await api.put<Playlist>(`/playlists/${playlist.id}`, body);
      playlist = { ...playlist, ...updated };
      success = t('playlists.updated');
      editing = false;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function deletePlaylist() {
    if (!playlist || !confirm(t('playlists.deleteConfirm'))) return;
    deleting = true;
    try {
      await api.delete(`/playlists/${playlist.id}`);
      window.location.href = "/playlists";
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      deleting = false;
    }
  }
</script>

<svelte:head><title>{playlist?.name ?? "Playlist"} — SoundTime</title></svelte:head>

{#if loading}
  <div class="flex justify-center py-20">
    <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
  </div>
{:else if playlist}
  <div class="space-y-6">
    {#if error}
      <div class="bg-red-500/10 border border-red-500/30 rounded-lg p-3 text-red-400 text-sm">{error}</div>
    {/if}
    {#if success}
      <div class="bg-green-500/10 border border-green-500/30 rounded-lg p-3 text-green-400 text-sm">{success}</div>
    {/if}

    <div class="flex items-end gap-6">
      {#if playlist.cover_url}
        <img src={playlist.cover_url} alt={playlist.name} class="w-48 h-48 rounded-lg shadow-xl object-cover flex-shrink-0" />
      {:else}
        <div class="w-48 h-48 rounded-lg bg-gradient-to-br from-[hsl(var(--primary))]/40 to-[hsl(var(--secondary))] flex items-center justify-center text-6xl shadow-xl flex-shrink-0">
          🎶
        </div>
      {/if}
      <div>
        <p class="text-xs uppercase tracking-wider text-[hsl(var(--muted-foreground))]">
          Playlist{#if !playlist.is_public} · <span class="text-yellow-400">{t('playlists.privateLabel')}</span>{/if}
        </p>
        <h1 class="text-4xl font-bold mt-1">{playlist.name}</h1>
        {#if playlist.description}
          <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">{playlist.description}</p>
        {/if}
        <p class="text-sm text-[hsl(var(--muted-foreground))] mt-2">
          {playlist.tracks?.length ?? 0} {t('playlists.tracks')}
        </p>
      </div>
    </div>

    <div class="flex gap-3">
      <button aria-label="Play all" class="w-12 h-12 rounded-full bg-[hsl(var(--primary))] text-white flex items-center justify-center hover:scale-105 transition" onclick={playAll}>
        <svg class="w-5 h-5 ml-0.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
      </button>

      {#if isOwner}
        <button
          class="px-4 py-2 bg-[hsl(var(--secondary))] hover:opacity-90 rounded-lg text-sm font-medium transition"
          onclick={() => { editing = !editing; success = null; }}
        >
          {editing ? t('playlists.cancelEdit') : t('playlists.edit')}
        </button>
        <button
          class="px-4 py-2 bg-red-500/20 text-red-400 hover:bg-red-500/30 rounded-lg text-sm font-medium transition"
          onclick={deletePlaylist}
          disabled={deleting}
        >
          {deleting ? t('playlists.deleting') : t('common.delete')}
        </button>
      {/if}
    </div>

    <!-- Edit Form -->
    {#if editing && isOwner}
      <form class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4" onsubmit={(e) => { e.preventDefault(); saveEdit(); }}>
        <div>
          <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-name">{t('playlists.nameLabel')}</label>
          <input id="edit-name" type="text" bind:value={editForm.name} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
        </div>
        <div>
          <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-desc">{t('playlists.descriptionLabel')}</label>
          <textarea id="edit-desc" bind:value={editForm.description} rows="3" class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))] resize-none"></textarea>
        </div>
        <div>
          <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-cover">{t('playlists.coverUrl')}</label>
          <input id="edit-cover" type="url" bind:value={editForm.cover_url} placeholder="https://..." class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
        </div>
        <div class="flex items-center gap-2">
          <input id="edit-public" type="checkbox" bind:checked={editForm.is_public} class="rounded" />
          <label class="text-sm" for="edit-public">{t('playlists.publicPlaylist')}</label>
        </div>
        <button type="submit" class="px-6 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium hover:opacity-90 transition">
          {t('common.save')}
        </button>
      </form>
    {/if}

    {#if playlist.tracks && playlist.tracks.length > 0}
      <TrackList tracks={playlist.tracks} />
    {:else}
      <p class="text-[hsl(var(--muted-foreground))] text-center py-10">{t('playlists.empty')}</p>
    {/if}
  </div>
{:else}
  <p class="text-center py-20 text-[hsl(var(--muted-foreground))]">{t('playlists.notFound')}</p>
{/if}
