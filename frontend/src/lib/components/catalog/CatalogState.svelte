<script lang="ts">
  import { Disc3, LoaderCircle, Unplug } from "lucide-svelte";
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";

  let { loading = false, failed = false, status, message, retry, children }: {
    loading?: boolean; failed?: boolean; status?: number; message?: string; retry?: () => void; children?: Snippet;
  } = $props();
</script>

<div class="flex min-h-44 flex-col items-center justify-center gap-3 rounded-xl border border-border bg-card px-6 py-10 text-center">
  <div class="text-muted-foreground" aria-hidden="true">
    {#if loading}<LoaderCircle size={23} class="motion-safe:animate-spin" />
    {:else if failed}<Unplug size={23} strokeWidth={1.5} />
    {:else}<Disc3 size={25} strokeWidth={1.5} />{/if}
  </div>
  <p role="status" class="max-w-md text-sm text-muted-foreground">
    {loading ? t("common.loading") : failed ? t(status === 403 ? "catalog.forbidden" : status === 404 ? "catalog.notFound" : "catalog.unavailable") : message}
  </p>
  {#if failed && retry}<Button variant="outline" onclick={retry}>{t("radio.tryAgain")}</Button>
  {:else if children}{@render children()}{/if}
</div>
