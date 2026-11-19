<script lang="ts">
  import { invalidateFavorites } from "$lib/catalog/favorites.svelte";
  import { api } from "$lib/api";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { trackId, size = 18, disabled = false, liked = $bindable(false) }: {
    trackId: string;
    size?: number;
    disabled?: boolean;
    liked?: boolean;
  } = $props();

  const auth = getAuthStore();
  let loading = $state(false);
  let failed = $state(false);

  async function toggle() {
    if (!auth.isAuthenticated || loading) return;
    loading = true;
    failed = false;
    try {
      if (liked) {
        await api.delete(`/favorites/${trackId}`);
        liked = false;
      } else {
        await api.post(`/favorites/${trackId}`);
        liked = true;
      }
      invalidateFavorites();
    } catch {
      failed = true;
    } finally {
      loading = false;
    }
  }
</script>

{#if auth.isAuthenticated}
  <button
    type="button" disabled={loading || disabled}
    aria-pressed={liked}
    class="grid min-h-8 min-w-8 place-items-center rounded-md hover:bg-secondary disabled:opacity-50"
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
  {#if failed}<span role="alert" class="absolute right-0 z-20 w-48 rounded-md border bg-popover p-2 text-xs text-destructive shadow">{t("favorites.updateFailed")}</span>{/if}
{/if}
