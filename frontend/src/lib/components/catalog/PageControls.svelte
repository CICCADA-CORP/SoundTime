<script lang="ts">
  import { ChevronLeft, ChevronRight } from "lucide-svelte";
  import { Button } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  let { page, total, select }: { page: number; total: number; select: (page: number) => void } = $props();
</script>

{#if total > 1}
  <div class="flex items-center justify-center gap-3 pt-2">
    <Button variant="outline" disabled={page <= 1} onclick={() => select(page - 1)} aria-label={t("admin.reports.previous")}><ChevronLeft size={16} /></Button>
    <span class="px-2 tabular-nums text-xs text-muted-foreground">{page} / {total}</span>
    <Button variant="outline" disabled={page >= total} onclick={() => select(page + 1)} aria-label={t("admin.reports.next")}><ChevronRight size={16} /></Button>
  </div>
{/if}
