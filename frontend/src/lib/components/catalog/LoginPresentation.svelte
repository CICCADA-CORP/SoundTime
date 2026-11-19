<script lang="ts">
  import { AudioLines } from "lucide-svelte";
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

<div class="account-card">
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
      <AudioLines aria-hidden="true" size={23} strokeWidth={1.8} />
      <div>
        <strong>{instanceName?.trim() || t("authShell.library")}</strong>
        <span>{instanceDescription?.trim() || t("authShell.libraryDetail")}</span>
      </div>
    </figcaption>
  </figure>
</div>

<style>
  .account-card {
    width: min(960px, 100%); min-height: 606px; min-width: 0;
    display: grid; grid-template-columns: minmax(0, .99fr) minmax(0, 1.01fr);
    border: 1px solid hsl(var(--border)); border-radius: 16px; background: hsl(var(--background));
    box-shadow: 0 2px 6px rgb(20 31 40 / .07), 0 1px 2px rgb(20 31 40 / .025);
  }
  .form-panel { min-width: 0; display: flex; flex-direction: column; padding: 38px clamp(26px, 3vw, 42px) 42px; }
  .brand { display: inline-flex; align-items: center; gap: 7px; align-self: flex-start; }
  .credentials { display: flex; flex-direction: column; flex: 1; padding: clamp(36px, 6.6vh, 62px) 0 20px; min-width: 0; }
  h1 { font-size: clamp(23px, 2vw, 27px); font-weight: 630; letter-spacing: -.045em; line-height: 1.17; text-wrap: balance; }
  .intro { margin: 6px 0 29px; color: hsl(var(--muted-foreground)); font-size: 14px; line-height: 1.5; }
  h1:last-of-type + :global(p[role="status"]) { margin-top: 14px; }
  .art-panel { position: relative; isolation: isolate; overflow: hidden; min-width: 0; margin: 6px 6px 6px 0; border-radius: 11px; background: #2f516a; }
  figcaption {
    position: absolute; z-index: 1; inset: auto 16px 16px; display: flex; align-items: center; gap: 12px;
    min-height: 61px; padding: 12px 15px; border: 1px solid rgb(255 255 255 / .24); border-radius: 11px;
    color: #102331; background: rgb(228 238 245 / .76); backdrop-filter: blur(14px);
  }
  figcaption :global(svg) { flex: none; }
  figcaption div { min-width: 0; overflow-wrap: anywhere; }
  figcaption strong { display: block; font-size: 14px; font-weight: 590; }
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
    .art-panel { height: clamp(125px, 19svh, 155px); margin: 0 6px 6px; }
    figcaption { inset: auto 10px 10px; min-height: 0; padding: 8px 11px; gap: 8px; }
  }
</style>
