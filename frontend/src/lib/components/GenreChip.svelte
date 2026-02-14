<script lang="ts">
  let { genre, active = false, onclick }: { genre: string; active?: boolean; onclick?: () => void } = $props();
</script>

<button
  class="px-4 py-1.5 rounded-full text-sm font-medium transition-all whitespace-nowrap
    {active
      ? 'bg-[hsl(var(--primary))] text-white shadow-md'
      : 'bg-[hsl(var(--secondary))] text-[hsl(var(--foreground))] hover:bg-[hsl(var(--border))]'}"
  {onclick}
>
  {genre}
</button>
