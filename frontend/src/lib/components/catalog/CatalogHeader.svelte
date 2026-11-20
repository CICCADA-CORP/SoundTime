<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, description, actions }: { title: string; description?: string; actions?: Snippet } = $props();
</script>

<header>
  <div class="heading">
    <h1>{title}</h1>
    {#if description}<p>{description}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  header { display: flex; align-items: end; justify-content: space-between; flex-wrap: wrap; gap: .8rem 1.25rem; }
  .heading { min-width: 0; }
  h1 { font-size: clamp(1.85rem, 3.6vw, 3rem); font-weight: 650; line-height: 1.08; letter-spacing: -.045em; overflow-wrap: anywhere; }
  p { margin-top: .35rem; max-width: 60ch; color: hsl(var(--muted-foreground)); font-size: .875rem; line-height: 1.5; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: .5rem; }
</style>
