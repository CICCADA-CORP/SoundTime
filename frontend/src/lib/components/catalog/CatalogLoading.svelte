<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import RecordShelf from "./RecordShelf.svelte";

  let { layout = "rows", mobileRow = false }: {
    layout?: "rows" | "shelf" | "detail"; mobileRow?: boolean;
  } = $props();
</script>

<div role="status">
  <span class="sr-only">{t("common.loading")}</span>
  <div aria-hidden="true">
    {#if layout === "shelf"}
      <RecordShelf {mobileRow}>
        {#each [1, 2, 3, 4] as position (position)}
          <div class="rounded-xl border border-border p-2">
            <div class="aspect-[8/5] rounded-lg bg-muted"></div>
            <div class="my-3 h-3 w-2/3 rounded bg-muted"></div>
            <div class="mb-2 h-2.5 w-1/3 rounded bg-muted"></div>
          </div>
        {/each}
      </RecordShelf>
    {:else}
      {#if layout === "detail"}
        <div class="mb-7 flex flex-col gap-5 sm:flex-row sm:items-end sm:gap-7">
          <div class="size-36 shrink-0 rounded-lg bg-muted sm:size-48"></div>
          <div class="flex flex-1 flex-col gap-3">
            <div class="h-3 w-16 rounded bg-muted"></div>
            <div class="h-8 w-3/5 max-w-80 rounded bg-muted"></div>
            <div class="h-4 w-2/5 max-w-44 rounded bg-muted"></div>
            <div class="h-9 w-24 rounded-md bg-muted"></div>
          </div>
        </div>
      {/if}
      {#each [1, 2, 3, 4] as position (position)}
        <div class="flex h-14 items-center gap-3 border-b border-border/60">
          <div class="size-9 shrink-0 rounded-md bg-muted"></div>
          <div class="flex flex-1 flex-col gap-2">
            <div class="h-2.5 w-2/5 max-w-56 rounded bg-muted"></div>
            <div class="h-2 w-1/4 max-w-32 rounded bg-muted"></div>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>
