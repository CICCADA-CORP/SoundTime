<script lang="ts">
  import { mountLiquidMetal } from "./renderer";

  let useVideo = $state(false);

  function mount(canvas: HTMLCanvasElement) {
    return mountLiquidMetal(canvas, (fallback) => { useVideo = fallback; });
  }
</script>

<div class="liquid-metal" class:video-fallback={useVideo} aria-hidden="true">
  <img src="/art/liquid-metal.webp" alt="" width="880" height="1100" />
  {#if useVideo}
    <video src="/art/liquid-metal.mp4" poster="/art/liquid-metal.webp" autoplay muted loop playsinline></video>
  {/if}
  <canvas {@attach mount}></canvas>
</div>

<style>
  .liquid-metal { position: absolute; inset: 0; pointer-events: none; background: #2d4f6c; }
  .liquid-metal > * { position: absolute; inset: 0; display: block; width: 100%; height: 100%; object-fit: cover; }
  canvas { opacity: 0; }
  canvas:global([data-ready]) { opacity: 1; }
  .video-fallback::after {
    content: ""; position: absolute; left: var(--pointer-x, 50%); top: var(--pointer-y, 50%);
    width: 72%; aspect-ratio: 1; transform: translate(-50%, -50%) rotate(-27deg);
    background: radial-gradient(ellipse 50% 33%, rgb(181 218 242 / .36), rgb(121 176 213 / .13) 48%, transparent 100%);
    mix-blend-mode: screen; opacity: var(--pointer-strength, 0);
    transition: left 110ms ease-out, top 110ms ease-out, opacity 240ms ease-out;
  }
</style>
