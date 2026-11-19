<script lang="ts">
  import { Podcast } from "lucide-svelte";
  import { stationUrl } from "./types";
  let { source }: { source: string | null } = $props();
  const image = $derived(stationUrl(source));
  const candidates = $derived.by(() => {
    if (!image) return undefined;
    const url = new URL(image);
    if (!/^is\d+(?:-ssl)?\.mzstatic\.com$/.test(url.hostname) || url.search || url.hash ||
        !url.pathname.startsWith("/image/thumb/") || !/\/\d+x\d+bb\.(?:jpg|jpeg|png|webp)$/i.test(url.pathname)) return undefined;
    return [120, 240, 480].map(width =>
      image.replace(/[^/]+$/, `${width}x${width}bb.webp`) + ` ${width}w`
    ).join(", ");
  });

  function recover(event: Event & { currentTarget: HTMLImageElement }) {
    const element = event.currentTarget;
    element.classList.remove("ready");
    if (element.hasAttribute("srcset")) element.removeAttribute("srcset");
  }
</script>

<span class="cover">
  {#if image}
    {#key image}
      <img src={image} srcset={candidates} sizes={candidates ? "auto, (max-width: 550px) 40vw, 200px" : undefined}
        alt="" loading="lazy" referrerpolicy="no-referrer"
        onload={e => e.currentTarget.classList.add("ready")} onerror={recover} />
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
