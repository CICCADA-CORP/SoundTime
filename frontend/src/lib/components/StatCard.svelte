<script lang="ts">
  import type { Snippet } from "svelte";

  let { value, label, icon }: { value: string | number; label: string; icon: Snippet } = $props();
</script>

<div class="bg-[hsl(var(--card))] border border-[hsl(var(--border))] rounded-xl p-4 flex items-center gap-3">
  <div class="w-10 h-10 rounded-lg bg-[hsl(var(--primary)/0.15)] text-[hsl(var(--primary))] flex items-center justify-center flex-shrink-0">
    {@render icon()}
  </div>
  <div class="min-w-0">
    <p class="text-lg font-bold leading-tight">{value}</p>
    <p class="text-xs text-[hsl(var(--muted-foreground))] truncate">{label}</p>
  </div>
</div>
