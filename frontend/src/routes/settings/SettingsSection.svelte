<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, children }: { title: string; children: Snippet } = $props();
</script>

<section>
  <h2>{title}</h2>
  <div class="controls">{@render children()}</div>
</section>

<style>
  section { display: grid; gap: 1rem 1.8rem; border-bottom: 1px solid hsl(var(--border)); padding-block: 1.5rem; }
  section:last-child { border-bottom-color: transparent; }
  h2 { font-size: 1.08rem; line-height: 1.35; font-weight: 620; letter-spacing: -.025em; }
  .controls { display: grid; min-width: 0; gap: 1rem; font-size: .875rem; overflow-wrap: anywhere; }
  @container settings (min-width: 43rem) { section { grid-template-columns: minmax(8rem, .64fr) minmax(0, 1.8fr); } }
</style>
