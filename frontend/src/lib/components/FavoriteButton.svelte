<script lang="ts">
  import { api } from "$lib/api";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { trackId, size = 18, liked = $bindable(false) }: {
    trackId: string;
    size?: number;
    liked?: boolean;
  } = $props();

  const auth = getAuthStore();
  let loading = $state(false);

  async function toggle() {
    if (!auth.isAuthenticated || loading) return;
    loading = true;
    try {
      if (liked) {
        await api.delete(`/favorites/${trackId}`);
        liked = false;
      } else {
        await api.post(`/favorites/${trackId}`);
        liked = true;
      }
    } catch {
      // silently ignore
    } finally {
      loading = false;
    }
  }
</script>

{#if auth.isAuthenticated}
  <button
    class="transition-all duration-200 hover:scale-110 {loading ? 'opacity-50 pointer-events-none' : ''}"
    onclick={(e) => { e.stopPropagation(); toggle(); }}
    title={liked ? t('favorites.remove') : t('favorites.add')}
  >
    {#if liked}
      <svg width={size} height={size} viewBox="0 0 24 24" fill="hsl(var(--primary))" stroke="hsl(var(--primary))" stroke-width="2">
        <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
      </svg>
    {:else}
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--primary))]">
        <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
      </svg>
    {/if}
  </button>
{/if}
