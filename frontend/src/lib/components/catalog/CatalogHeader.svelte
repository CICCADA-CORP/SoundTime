<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, description, actions }: { title: string; description?: string; actions?: Snippet } = $props();
</script>

<header class="flex flex-wrap items-end justify-between gap-4">
  <div class="min-w-0">
    <h1 class="text-2xl font-semibold leading-tight tracking-tight">{title}</h1>
    {#if description}<p class="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">{description}</p>{/if}
  </div>
  {#if actions}{@render actions()}{/if}
</header>
