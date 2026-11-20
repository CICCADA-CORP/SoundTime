<script lang="ts">
  import { Antenna } from "lucide-svelte";
  import { stationUrl } from "./types";
  let { source }: { source: string | null } = $props();
  const icon = $derived(stationUrl(source));
</script>
<span class="relative grid size-full place-items-center overflow-hidden rounded-lg border border-border/50 bg-muted text-muted-foreground">
  {#if icon}
    {#key icon}
      <img src={icon} alt="" referrerpolicy="no-referrer" loading="lazy"
        class="peer absolute inset-0 size-full object-contain opacity-0 data-[loaded]:opacity-100"
        onload={(event) => event.currentTarget.setAttribute("data-loaded", "")}
        onerror={(event) => event.currentTarget.removeAttribute("data-loaded")} />
    {/key}
  {/if}
  <Antenna size={23} strokeWidth={1.5} aria-hidden="true" class="peer-data-[loaded]:invisible" />
</span>
