<script lang="ts">
  import { Podcast } from "lucide-svelte";
  import { stationUrl } from "./types";
  let { source }: { source: string | null } = $props();
  const image = $derived(stationUrl(source));
</script>

<span class="cover">
  {#if image}
    {#key image}
      <img src={image} alt="" loading="lazy" referrerpolicy="no-referrer"
        onload={e => e.currentTarget.classList.add("ready")} onerror={e => e.currentTarget.classList.remove("ready")} />
    {/key}
  {/if}
  <Podcast size={32} strokeWidth={1.4} aria-hidden="true" />
</span>

<style>
  .cover { position: relative; display: grid; place-items: center; width: 100%; aspect-ratio: 1; overflow: hidden; border-radius: .85rem; background: linear-gradient(135deg, #723927, #38273c 75%); color: #fae2c1; }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; opacity: 0; }
  img:global(.ready) { opacity: 1; }
  img:global(.ready) + :global(svg) { visibility: hidden; }
</style>
