<script lang="ts">
  import type { Snippet } from "svelte";
  let { children }: { children: Snippet } = $props();
</script>

<div class="flex min-h-screen items-center justify-center bg-sidebar px-4 text-foreground">
  {@render children()}
</div>
