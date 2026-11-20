<script lang="ts">
  import type { Snippet } from "svelte";
  let { children }: { children: Snippet } = $props();
</script>

<div class="flex min-h-svh items-center justify-center px-4 py-4 text-foreground">
  {@render children()}
</div>
