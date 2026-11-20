<script lang="ts">
  let { source, title, kind, artworkId, loading = "lazy" }: {
    source?: string | null; title: string; kind: "album" | "artist" | "playlist"; artworkId?: string | null; loading?: "eager" | "lazy";
  } = $props();
  let broken = $state<string | null>(null);
  let loaded = $state<string | null>(null);
  const showImage = $derived(Boolean(source && broken !== source));
  const hues = [208, 244, 270, 335, 166, 191, 25, 156];
  const seed = $derived(Array.from(artworkId || title).reduce((hash, char) => (Math.imul(hash, 31) + char.charCodeAt(0)) >>> 0, 17));
  const hue = $derived(hues[seed % hues.length]);
  // Each four-bit group determines one bar's height on this cover.
  const levels = $derived([0, 4, 8, 12, 16, 20, 24].map(shift => 25 + ((seed >>> shift) & 15) * 5));
  const initials = $derived(title.trim().split(/\s+/u).slice(0, 2).map(word => Array.from(word)[0]).join("").toLocaleUpperCase());
</script>

<span class="artwork {kind}">
  {#if !showImage || loaded !== source}
    <span role={showImage ? undefined : "img"} aria-hidden={showImage || undefined} aria-label={showImage ? undefined : title}
      class="fallback" style:--hue={hue}>
      {#if kind === "artist" && initials}
        <span class="initials" aria-hidden="true">{initials}</span>
      {:else}
        <span class="wave" aria-hidden="true" class:reverse={(seed & 8) !== 0}>
          {#each levels as level, index (index)}<i style:height={level + "%"}></i>{/each}
        </span>
      {/if}
    </span>
  {/if}
  {#if showImage && source}
    <img src={source} alt={title} {loading} decoding="async" class:ready={loaded === source}
      onload={event => loaded = event.currentTarget.getAttribute("src")}
      onerror={event => broken = event.currentTarget.getAttribute("src")} />
  {/if}
</span>

<style>
  .artwork { display: grid; width: 100%; height: 100%; overflow: hidden; position: relative; border-radius: inherit; background: hsl(var(--muted)); }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; opacity: 0; }
  img.ready { opacity: 1; }
  .fallback { position: absolute; inset: 0; display: grid; place-items: center; container-type: inline-size;
    background: linear-gradient(146deg, hsl(var(--hue) 31% 85%), hsl(calc(var(--hue) + 8) 37% 60%)); }
  .wave { display: flex; width: 70%; height: 65%; align-items: center; gap: 3%; transform: rotate(-16deg); }
  .wave.reverse { transform: rotate(16deg); }
  .wave i { flex: 1; border-radius: 999px; background: linear-gradient(180deg, hsl(var(--hue) 43% 93% / .78), hsl(var(--hue) 34% 38% / .42)); }
  .artist .fallback { color: hsl(var(--hue) 32% 27%); background: linear-gradient(145deg, hsl(var(--hue) 28% 94%), hsl(var(--hue) 31% 80%)); }
  .initials { font-size: 32cqi; font-weight: 550; line-height: 1; letter-spacing: -.06em; }
</style>
