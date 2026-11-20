<script lang="ts">
  let { genre }: { genre: string } = $props();
</script>

<span class="genre">{genre}</span>

<style>
  .genre { display: inline-flex; max-width: 100%; align-items: center; min-height: 2.2rem; padding: .35rem .9rem;
    border: 1px solid hsl(var(--border)); border-radius: 999px; background: hsl(var(--card)); color: hsl(var(--foreground));
    font-size: .83rem; font-weight: 520; line-height: 1.3; overflow-wrap: anywhere; text-align: start; }
  :global(a:hover) > .genre, :global(button:hover) > .genre { background: hsl(var(--muted)); }
  :global(button[aria-pressed="true"]) > .genre { color: hsl(var(--primary)); border-color: hsl(var(--primary) / .55); background: hsl(var(--primary) / .09); }
</style>
