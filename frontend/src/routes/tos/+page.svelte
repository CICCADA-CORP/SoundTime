<script lang="ts">
  import { api } from "$lib/api";
  import type { TosResponse } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";

  const terms = catalogQuery(() => api.get<TosResponse>("/tos"));
  const source = $derived(terms.data?.content.trim() ?? "");
  const heading = $derived(/^# (.+)(?:\r?\n|$)/.exec(source));
  const body = $derived(heading ? source.slice(heading[0].length) : source);

  type Block =
    | { kind: "h2" | "h3" | "p"; text: string }
    | { kind: "list"; items: string[] }
    | { kind: "rule" };

  function parseBlocks(source: string): Block[] {
    const blocks: Block[] = [];
    let items: string[] | null = null;
    for (const line of source.split(/\r?\n/)) {
      if (line.startsWith("- ")) {
        if (!items) {
          items = [];
          blocks.push({ kind: "list", items });
        }
        items.push(line.slice(2));
      } else {
        items = null;
        if (line.startsWith("### ")) blocks.push({ kind: "h3", text: line.slice(4) });
        else if (/^#{1,2} /.test(line)) blocks.push({ kind: "h2", text: line.replace(/^#{1,2} /, "") });
        else if (line.trim() === "---") blocks.push({ kind: "rule" });
        else if (line.trim()) blocks.push({ kind: "p", text: line });
      }
    }
    return blocks;
  }
  const blocks = $derived(parseBlocks(body));
</script>

{#snippet inline(value: string)}
  {#each value.split(/(\*\*[^*]+\*\*|\*[^*]+\*)/g) as part, index (index)}
    {#if index % 2 === 0}{part}{:else if part.startsWith("**")}<strong>{part.slice(2, -2)}</strong>{:else}<em>{part.slice(1, -1)}</em>{/if}
  {/each}
{/snippet}

<svelte:head><title>{t("tos.title")}</title></svelte:head>
<div class="max-w-3xl space-y-6">
  <CatalogHeader title={heading?.[1] ?? t("authShell.terms")} />
  {#if terms.loading || terms.failed}
    <CatalogState loading={terms.loading} failed={terms.failed} retry={terms.retry} />
  {:else}
    <article class="terms">
      {#each blocks as block, index (index)}
        {#if block.kind === "list"}
          <ul>{#each block.items as item, position (position)}<li>{@render inline(item)}</li>{/each}</ul>
        {:else if block.kind === "rule"}
          <hr>
        {:else}
          <svelte:element this={block.kind}>{@render inline(block.text)}</svelte:element>
        {/if}
      {/each}
    </article>
  {/if}
</div>

<style>
  .terms { font-size: .9375rem; line-height: 1.7; overflow-wrap: anywhere; }
  .terms :global(p), .terms ul { margin: .6rem 0; }
  .terms ul { list-style: disc; padding-inline-start: 1.5rem; }
  .terms :global(h2), .terms :global(h3) { margin-block: 1.4rem .4rem; font-weight: 600; }
  .terms :global(h2) { font-size: 1.125rem; }
  .terms hr { margin-block: 1.5rem; border-color: hsl(var(--border)); }
</style>
