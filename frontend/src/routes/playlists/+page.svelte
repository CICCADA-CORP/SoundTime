<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Playlist } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Music, Plus, Lock, Globe, Trash2, X } from "lucide-svelte";

  const auth = getAuthStore();
  let playlists: Playlist[] = $state([]);
  let loading = $state(true);
  let showCreateModal = $state(false);
  let newName = $state("");
  let newDescription = $state("");
  let newIsPublic = $state(true);
  let creating = $state(false);
  let deletingId = $state<string | null>(null);

  onMount(async () => {
    await loadPlaylists();
  });

  async function loadPlaylists() {
    loading = true;
    try {
      const res = await api.get<{ data: Playlist[] }>("/playlists");
      playlists = res.data ?? [];
    } catch { /* empty */ } finally { loading = false; }
  }

  async function createPlaylist() {
    if (!newName.trim()) return;
    creating = true;
    try {
      await api.post("/playlists", {
        name: newName.trim(),
        description: newDescription.trim() || null,
        is_public: newIsPublic,
      });
      newName = "";
      newDescription = "";
      newIsPublic = true;
      showCreateModal = false;
      await loadPlaylists();
    } catch { /* empty */ } finally { creating = false; }
  }

  async function deletePlaylist(id: string) {
    if (!confirm(t('playlists.deleteConfirm'))) return;
    deletingId = id;
    try {
      await api.delete(`/playlists/${id}`);
      await loadPlaylists();
    } catch { /* empty */ } finally { deletingId = null; }
  }
</script>

<svelte:head><title>{t('playlists.title')} — SoundTime</title></svelte:head>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold">{t('playlists.title')}</h1>
    {#if auth.isAuthenticated}
      <button
        class="flex items-center gap-2 px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium hover:opacity-90 transition"
        onclick={() => showCreateModal = true}
      >
        <Plus size={16} />
        {t('playlists.create')}
      </button>
    {/if}
  </div>

  {#if loading}
    <div class="flex justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else if playlists.length > 0}
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
      {#each playlists as playlist}
        <div class="group relative bg-[hsl(var(--card))] rounded-xl overflow-hidden border border-[hsl(var(--border))] hover:border-[hsl(var(--primary)/0.5)] transition-all hover:shadow-lg">
          <a href="/playlists/{playlist.id}" class="block">
            <div class="aspect-square bg-gradient-to-br from-[hsl(var(--primary)/0.3)] via-[hsl(var(--primary)/0.1)] to-[hsl(var(--secondary))] flex items-center justify-center relative">
              {#if playlist.cover_url}
                <img src={playlist.cover_url} alt={playlist.name} class="w-full h-full object-cover" />
              {:else}
                <Music size={48} class="text-[hsl(var(--muted-foreground)/0.4)]" />
              {/if}
              <!-- Visibility badge -->
              <div class="absolute top-2 right-2">
                {#if playlist.is_public}
                  <span class="flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-green-500/20 text-green-400 backdrop-blur-sm">
                    <Globe size={10} />
                    {t('playlists.public')}
                  </span>
                {:else}
                  <span class="flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-yellow-500/20 text-yellow-400 backdrop-blur-sm">
                    <Lock size={10} />
                    {t('playlists.private')}
                  </span>
                {/if}
              </div>
            </div>
            <div class="p-4">
              <h3 class="font-semibold text-sm truncate group-hover:text-[hsl(var(--primary))] transition-colors">{playlist.name}</h3>
              {#if playlist.description}
                <p class="text-xs text-[hsl(var(--muted-foreground))] line-clamp-1 mt-0.5">{playlist.description}</p>
              {/if}
              <p class="text-xs text-[hsl(var(--muted-foreground))] mt-1">
                {playlist.track_count ?? 0} {t('playlists.tracks')}
              </p>
            </div>
          </a>
          {#if auth.isAuthenticated}
            <button
              class="absolute top-2 left-2 p-1.5 rounded-lg bg-red-500/20 text-red-400 opacity-0 group-hover:opacity-100 transition-opacity hover:bg-red-500/30"
              onclick={(e) => { e.preventDefault(); e.stopPropagation(); deletePlaylist(playlist.id); }}
              disabled={deletingId === playlist.id}
              title={t('common.delete')}
            >
              <Trash2 size={14} />
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {:else}
    <div class="text-center py-16">
      <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center">
        <Music size={32} class="text-[hsl(var(--muted-foreground))]" />
      </div>
      <p class="text-[hsl(var(--muted-foreground))]">{t('playlists.noPlaylists')}</p>
      {#if auth.isAuthenticated}
        <button class="mt-4 px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium" onclick={() => showCreateModal = true}>
          {t('playlists.createFirst')}
        </button>
      {/if}
    </div>
  {/if}
</div>

<!-- Create Playlist Modal -->
{#if showCreateModal}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 bg-black/60 z-50 flex items-center justify-center p-4" onclick={() => showCreateModal = false} onkeydown={(e) => e.key === 'Escape' && (showCreateModal = false)} role="dialog" tabindex="-1">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bg-[hsl(var(--card))] rounded-xl p-6 w-full max-w-md shadow-2xl space-y-4" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="flex items-center justify-between">
        <h3 class="text-lg font-semibold">{t('playlists.createNew')}</h3>
        <button class="p-1 rounded hover:bg-[hsl(var(--secondary))] transition" onclick={() => showCreateModal = false}>
          <X size={18} class="text-[hsl(var(--muted-foreground))]" />
        </button>
      </div>
      <form class="space-y-4" onsubmit={(e) => { e.preventDefault(); createPlaylist(); }}>
        <div>
          <label for="playlist-name" class="text-sm block mb-1">{t('playlists.nameLabel')}</label>
          <input id="playlist-name" type="text" bind:value={newName} placeholder={t('playlists.newName')} class="w-full bg-[hsl(var(--secondary))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:ring-2 focus:ring-[hsl(var(--primary))] outline-none" />
        </div>
        <div>
          <label for="playlist-desc" class="text-sm block mb-1">{t('playlists.descriptionLabel')}</label>
          <textarea id="playlist-desc" bind:value={newDescription} placeholder={t('playlists.descriptionPlaceholder')} rows="2" class="w-full bg-[hsl(var(--secondary))] rounded-lg px-3 py-2 text-sm border border-[hsl(var(--border))] focus:ring-2 focus:ring-[hsl(var(--primary))] outline-none resize-none"></textarea>
        </div>
        <div class="flex items-center gap-3">
          <button type="button" class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm transition {newIsPublic ? 'bg-green-500/20 text-green-400 ring-1 ring-green-500/40' : 'bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))]'}" onclick={() => newIsPublic = true}>
            <Globe size={14} />
            {t('playlists.public')}
          </button>
          <button type="button" class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm transition {!newIsPublic ? 'bg-yellow-500/20 text-yellow-400 ring-1 ring-yellow-500/40' : 'bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))]'}" onclick={() => newIsPublic = false}>
            <Lock size={14} />
            {t('playlists.private')}
          </button>
        </div>
        <div class="flex gap-2 justify-end">
          <button type="button" class="px-4 py-2 text-sm rounded-lg hover:bg-[hsl(var(--secondary))] transition" onclick={() => showCreateModal = false}>
            {t('common.cancel')}
          </button>
          <button type="submit" disabled={creating || !newName.trim()} class="px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium disabled:opacity-50">
            {creating ? t('common.loading') : t('playlists.create')}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}
