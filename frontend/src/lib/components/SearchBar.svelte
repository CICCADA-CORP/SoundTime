<script lang="ts">
  import { goto } from "$app/navigation";
  import { t } from "$lib/i18n/index.svelte";

  let query = "";

  function handleSubmit() {
    if (query.trim()) {
      goto(`/search?q=${encodeURIComponent(query.trim())}`);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") handleSubmit();
  }
</script>

<div class="relative max-w-md w-full">
  <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[hsl(var(--muted-foreground))]" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
  </svg>
  <input
    type="text"
    bind:value={query}
    onkeydown={handleKeydown}
    placeholder={t('search.placeholder')}
    class="w-full pl-10 pr-4 py-2 rounded-full bg-[hsl(var(--secondary))] text-sm text-white placeholder-[hsl(var(--muted-foreground))] border-none outline-none focus:ring-2 focus:ring-[hsl(var(--primary))] transition"
  />
</div>
