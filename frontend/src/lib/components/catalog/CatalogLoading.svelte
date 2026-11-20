<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import RecordShelf from "./RecordShelf.svelte";
  import ReleaseFrame from "./ReleaseFrame.svelte";

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
          <div class="min-w-0">
            <div class="aspect-square rounded-[var(--art-radius)] bg-muted"></div>
            <div class="my-2 h-3.5 w-2/3 rounded bg-muted"></div>
            <div class="mb-2 h-2.5 w-1/3 rounded bg-muted"></div>
          </div>
        {/each}
      </RecordShelf>
    {:else}
      {#if layout === "detail"}
        <div class="mb-6"><ReleaseFrame>
          <div class="grid gap-3">
            <div class="h-3 w-16 bg-muted"></div>
            <div class="h-10 w-3/5 max-w-80 bg-muted"></div>
            <div class="h-4 w-2/5 max-w-44 bg-muted"></div>
            <div class="h-9 w-24 bg-muted"></div>
          </div>
        </ReleaseFrame></div>
      {/if}
      {#each [1, 2, 3, 4] as position (position)}
        <div class="flex h-12 items-center gap-3 border-b border-border/60">
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
