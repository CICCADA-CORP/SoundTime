<script lang="ts">
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { TrackCredits } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { formatDuration } from "$lib/utils";

  const auth = getAuthStore();
  const player = getPlayerStore();
  let credits: TrackCredits | null = null;
  let loading = true;
  let editing = false;
  let deleting = false;
  let editForm = { title: "", genre: "", year: "" as string, track_number: "" as string, disc_number: "" as string };
  let error: string | null = null;
  let success: string | null = null;
  let showReportForm = false;
  let reportReason = "";
  let reportError = "";
  let reportSuccess = "";

  $: isOwner = credits?.uploaded_by && auth.user?.id === credits.uploaded_by;

  onMount(async () => {
    const id = $page.params.id;
    try {
      credits = await api.get<TrackCredits>(`/tracks/${id}/credits`);
      if (credits) {
        editForm = {
          title: credits.title,
          genre: credits.genre ?? "",
          year: credits.year?.toString() ?? "",
          track_number: credits.track_number?.toString() ?? "",
          disc_number: credits.disc_number?.toString() ?? "",
        };
      }
    } catch { /* empty */ } finally { loading = false; }
  });

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
      success = "Métadonnées mises à jour";
      editing = false;
      // Reload
      credits = await api.get<TrackCredits>(`/tracks/${credits.id}/credits`);
    } catch (e: any) {
      error = e.message;
    }
  }

  async function deleteTrack() {
    if (!credits || !confirm("Supprimer ce morceau ? Cette action est irréversible.")) return;
    deleting = true;
    try {
      await api.delete(`/tracks/${credits.id}`);
      window.location.href = "/explore";
    } catch (e: any) {
      error = e.message;
      deleting = false;
    }
  }

  async function submitReport() {
    if (!credits || !reportReason.trim()) return;
    reportError = "";
    try {
      await api.post(`/tracks/${credits.id}/report`, { reason: reportReason.trim() });
      reportSuccess = "Signalement envoyé. L'administrateur examinera votre demande.";
      reportReason = "";
      setTimeout(() => { showReportForm = false; reportSuccess = ""; }, 3000);
    } catch (e: any) {
      reportError = e?.message ?? "Erreur lors du signalement.";
    }
  }
</script>

<svelte:head><title>{credits?.title ?? "Track"} — Crédits — SoundTime</title></svelte:head>

{#if loading}
  <div class="flex justify-center py-20">
    <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
  </div>
{:else if credits}
  <div class="max-w-3xl mx-auto space-y-8">
    <!-- Header -->
    <div class="flex items-end gap-6">
      {#if credits.album_cover_url}
        <img src={credits.album_cover_url} alt={credits.album_title ?? ""} class="w-48 h-48 rounded-lg shadow-xl object-cover flex-shrink-0" />
      {:else}
        <div class="w-48 h-48 rounded-lg bg-gradient-to-br from-[hsl(var(--primary))]/40 to-[hsl(var(--secondary))] flex items-center justify-center text-6xl shadow-xl flex-shrink-0">🎵</div>
      {/if}
      <div class="min-w-0">
        <p class="text-xs uppercase tracking-wider text-[hsl(var(--muted-foreground))]">Morceau</p>
        <h1 class="text-3xl font-bold mt-1 truncate">{credits.title}</h1>
        <a href="/artists/{credits.artist_id}" class="text-lg text-[hsl(var(--muted-foreground))] hover:text-white transition">{credits.artist_name}</a>
        {#if credits.album_title}
          <p class="text-sm text-[hsl(var(--muted-foreground))] mt-1">
            Album: <a href="/albums/{credits.album_id}" class="hover:text-white transition">{credits.album_title}</a>
          </p>
        {/if}
        <p class="text-xs text-[hsl(var(--muted-foreground))] mt-2">
          {credits.play_count} écoute{credits.play_count !== 1 ? "s" : ""}
        </p>
      </div>
    </div>

    <!-- Actions -->
    {#if error}
      <div class="bg-red-500/10 border border-red-500/30 rounded-lg p-3 text-red-400 text-sm">{error}</div>
    {/if}
    {#if success}
      <div class="bg-green-500/10 border border-green-500/30 rounded-lg p-3 text-green-400 text-sm">{success}</div>
    {/if}

    {#if isOwner}
      <div class="flex gap-3">
        <button
          class="px-4 py-2 bg-[hsl(var(--secondary))] hover:opacity-90 rounded-lg text-sm font-medium transition"
          on:click={() => { editing = !editing; success = null; }}
        >
          {editing ? "Annuler" : "Modifier les métadonnées"}
        </button>
        <button
          class="px-4 py-2 bg-red-500/20 text-red-400 hover:bg-red-500/30 rounded-lg text-sm font-medium transition"
          on:click={deleteTrack}
          disabled={deleting}
        >
          {deleting ? "Suppression..." : "Supprimer le morceau"}
        </button>
      </div>
    {/if}

    <!-- Report button (for any authenticated user) -->
    {#if auth.user && !isOwner}
      <div>
        {#if reportSuccess}
          <p class="text-green-400 text-sm bg-green-500/10 border border-green-500/30 rounded-lg p-3">{reportSuccess}</p>
        {:else if showReportForm}
          <div class="bg-[hsl(var(--card))] rounded-lg p-5 space-y-3 border border-[hsl(var(--border))]">
            <h3 class="text-sm font-semibold">Signaler cette piste</h3>
            {#if reportError}
              <p class="text-red-400 text-sm">{reportError}</p>
            {/if}
            <textarea
              bind:value={reportReason}
              placeholder="Décrivez le problème (ex: violation de droits d'auteur, contenu inapproprié...)"
              class="w-full px-3 py-2 rounded-lg bg-[hsl(var(--secondary))] text-sm resize-none h-24 border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]"
              maxlength="500"
            ></textarea>
            <p class="text-xs text-[hsl(var(--muted-foreground))] text-right">{reportReason.length}/500</p>
            <div class="flex gap-3">
              <button class="px-4 py-2 text-sm rounded-lg bg-[hsl(var(--secondary))] hover:opacity-80 transition" on:click={() => showReportForm = false}>Annuler</button>
              <button
                class="px-4 py-2 text-sm rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 font-medium transition disabled:opacity-50"
                disabled={!reportReason.trim()}
                on:click={submitReport}
              >Envoyer</button>
            </div>
          </div>
        {:else}
          <button
            class="px-4 py-2 text-sm rounded-lg border border-[hsl(var(--border))] text-[hsl(var(--muted-foreground))] hover:text-red-400 hover:border-red-400/30 transition flex items-center gap-2"
            on:click={() => showReportForm = true}
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3 3v18m0-18l9 6-9 6"/></svg>
            Signaler cette piste
          </button>
        {/if}
      </div>
    {/if}

    <!-- Edit Form -->
    {#if editing && isOwner}
      <form class="bg-[hsl(var(--card))] rounded-lg p-6 space-y-4" on:submit|preventDefault={saveEdit}>
        <div>
          <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-title">Titre</label>
          <input id="edit-title" type="text" bind:value={editForm.title} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
        </div>
        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-genre">Genre</label>
            <input id="edit-genre" type="text" bind:value={editForm.genre} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-year">Année</label>
            <input id="edit-year" type="number" bind:value={editForm.year} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
        </div>
        <div class="grid grid-cols-2 gap-4">
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-track">N° piste</label>
            <input id="edit-track" type="number" bind:value={editForm.track_number} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
          <div>
            <label class="text-xs text-[hsl(var(--muted-foreground))] block mb-1" for="edit-disc">N° disque</label>
            <input id="edit-disc" type="number" bind:value={editForm.disc_number} class="w-full px-3 py-2 rounded bg-[hsl(var(--secondary))] text-sm border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))]" />
          </div>
        </div>
        <button type="submit" class="px-6 py-2 bg-[hsl(var(--primary))] text-white rounded-lg text-sm font-medium hover:opacity-90 transition">
          Enregistrer
        </button>
      </form>
    {/if}

    <!-- Metadata Table -->
    <div class="bg-[hsl(var(--card))] rounded-lg overflow-hidden">
      <h2 class="text-lg font-semibold p-5 border-b border-[hsl(var(--border))]">Informations techniques</h2>
      <div class="divide-y divide-[hsl(var(--border))]">
        <div class="grid grid-cols-2 p-4">
          <span class="text-sm text-[hsl(var(--muted-foreground))]">Durée</span>
          <span class="text-sm">{formatDuration(credits.duration_secs)}</span>
        </div>
        <div class="grid grid-cols-2 p-4">
          <span class="text-sm text-[hsl(var(--muted-foreground))]">Format</span>
          <span class="text-sm uppercase">{credits.format}</span>
        </div>
        {#if credits.bitrate}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">Bitrate</span>
            <span class="text-sm">{credits.bitrate} kbps</span>
          </div>
        {/if}
        {#if credits.sample_rate}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">Fréquence d'échantillonnage</span>
            <span class="text-sm">{(credits.sample_rate / 1000).toFixed(1)} kHz</span>
          </div>
        {/if}
        {#if credits.genre}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">Genre</span>
            <span class="text-sm">{credits.genre}</span>
          </div>
        {/if}
        {#if credits.year}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">Année</span>
            <span class="text-sm">{credits.year}</span>
          </div>
        {/if}
        {#if credits.track_number}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">Piste</span>
            <span class="text-sm">{credits.track_number}{credits.disc_number ? ` (Disque ${credits.disc_number})` : ""}</span>
          </div>
        {/if}
        {#if credits.musicbrainz_id}
          <div class="grid grid-cols-2 p-4">
            <span class="text-sm text-[hsl(var(--muted-foreground))]">MusicBrainz ID</span>
            <a href="https://musicbrainz.org/recording/{credits.musicbrainz_id}" target="_blank" rel="noopener" class="text-sm text-[hsl(var(--primary))] hover:underline font-mono">{credits.musicbrainz_id.slice(0,8)}…</a>
          </div>
        {/if}
        <div class="grid grid-cols-2 p-4">
          <span class="text-sm text-[hsl(var(--muted-foreground))]">Ajouté le</span>
          <span class="text-sm">{new Date(credits.created_at).toLocaleDateString("fr-FR", { year: "numeric", month: "long", day: "numeric" })}</span>
        </div>
      </div>
    </div>

    <!-- Artist Info -->
    <div class="bg-[hsl(var(--card))] rounded-lg overflow-hidden">
      <h2 class="text-lg font-semibold p-5 border-b border-[hsl(var(--border))]">Artiste</h2>
      <div class="p-5 flex items-center gap-4">
        {#if credits.artist_image}
          <img src={credits.artist_image} alt={credits.artist_name} class="w-16 h-16 rounded-full object-cover" />
        {:else}
          <div class="w-16 h-16 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center text-2xl">🎤</div>
        {/if}
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
      <div class="bg-[hsl(var(--card))] rounded-lg overflow-hidden">
        <h2 class="text-lg font-semibold p-5 border-b border-[hsl(var(--border))]">Album</h2>
        <div class="p-5 flex items-center gap-4">
          {#if credits.album_cover_url}
            <img src={credits.album_cover_url} alt={credits.album_title ?? ""} class="w-16 h-16 rounded object-cover" />
          {:else}
            <div class="w-16 h-16 rounded bg-[hsl(var(--secondary))] flex items-center justify-center text-2xl">💿</div>
          {/if}
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
  <p class="text-center py-20 text-[hsl(var(--muted-foreground))]">Morceau introuvable.</p>
{/if}
