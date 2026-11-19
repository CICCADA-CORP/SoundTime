<script lang="ts">
  import { Antenna } from "lucide-svelte";
  import { stationUrl } from "./types";
  let { source }: { source: string | null } = $props();
  let broken = $state<string | null>(null);
  const icon = $derived(stationUrl(source));
</script>
<span class="grid size-full place-items-center overflow-hidden rounded-lg border border-border/50 bg-muted text-muted-foreground">
  {#if icon && icon !== broken}
    <img src={icon} alt="" referrerpolicy="no-referrer" loading="lazy" class="size-full object-contain" onerror={() => broken = icon} />
  {:else}<Antenna size={23} strokeWidth={1.5} aria-hidden="true" />{/if}
</span>
