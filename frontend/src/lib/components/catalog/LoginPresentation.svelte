<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import SoundTimeSignature from "../SoundTimeSignature.svelte";
  import LiquidMetal from "./liquid-metal/LiquidMetal.svelte";

  let { children, title, subtitle, instanceName, instanceDescription }: {
    children: Snippet;
    title: string;
    subtitle?: string;
    instanceName?: string;
    instanceDescription?: string;
  } = $props();
</script>

<div class="account-card glass-surface">
  <div class="form-panel">
    <div class="brand">
      <SoundTimeSignature width={132} />
    </div>
    <section class="credentials" aria-labelledby="account-title">
      <h1 id="account-title">{title}</h1>
      {#if subtitle}<p class="intro">{subtitle}</p>{/if}
      {@render children()}
    </section>
  </div>
  <figure class="art-panel">
    <LiquidMetal />
    <figcaption>
      <div>
        <strong>{instanceName?.trim() || t("authShell.library")}</strong>
        <span>{instanceDescription?.trim() || t("authShell.libraryDetail")}</span>
      </div>
    </figcaption>
  </figure>
</div>

<style>
  .account-card {
    width: min(920px, 100%); min-height: 530px; min-width: 0;
    display: grid; grid-template-columns: repeat(2, minmax(0, 1fr));
    border-radius: 1.35rem; backdrop-filter: blur(24px) saturate(115%);
  }
  .form-panel { min-width: 0; display: flex; flex-direction: column; padding: 38px clamp(26px, 3vw, 42px) 42px; }
  .brand { display: flex; align-items: center; }
  .credentials { display: flex; flex-direction: column; flex: 1; padding: clamp(36px, 6.6vh, 62px) 0 20px; min-width: 0; }
  h1 { font-size: clamp(1.55rem, 2.5vw, 2.05rem); font-weight: 650; letter-spacing: -.04em; line-height: 1.15; text-wrap: balance; }
  .intro { margin: 6px 0 29px; color: hsl(var(--muted-foreground)); font-size: 14px; line-height: 1.5; }
  h1:last-of-type + :global(p[role="status"]) { margin-top: 14px; }
  .art-panel { position: relative; isolation: isolate; overflow: hidden; min-width: 0; margin: .5rem .5rem .5rem 0; border-radius: .98rem; background: #2f516a; }
  figcaption {
    position: absolute; z-index: 1; inset: auto .9rem .9rem; padding: .8rem 1rem; border-radius: .7rem;
    color: #152a3d; background: rgb(240 247 253 / .88); backdrop-filter: blur(12px);
  }
  figcaption div { min-width: 0; overflow-wrap: anywhere; }
  figcaption strong { display: block; margin-bottom: .15rem; font-size: .86rem; font-weight: 620; line-height: 1.4; }
  figcaption span { display: -webkit-box; overflow: hidden; line-clamp: 3; -webkit-line-clamp: 3; -webkit-box-orient: vertical; font-size: 12px; line-height: 1.4; }
  @media (min-width: 681px) and (max-height: 740px) {
    .account-card { min-height: 490px; }
    .form-panel { padding-block: 28px; }
    .credentials { padding-block: 22px; }
  }
  @media (max-width: 680px) {
    .account-card { width: min(435px, 100%); min-height: 0; grid-template-columns: minmax(0, 1fr); }
    .form-panel { padding: 25px 23px 27px; }
    .credentials { padding: 28px 0 0; }
    .intro { margin-bottom: 21px; }
    .art-panel { height: clamp(150px, 23svh, 190px); margin: 0 .4rem .4rem; }
    figcaption { inset: auto .6rem .6rem; padding: .5rem .72rem; }
  }
</style>
