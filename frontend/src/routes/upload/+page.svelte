<script lang="ts">
  import { signInLink } from "$lib/access/navigation";
  import { page } from "$app/state";
  import { CircleCheck } from "lucide-svelte";
  import { Button } from "$lib/components/ui/button";
  import CatalogHeader from "$lib/components/catalog/CatalogHeader.svelte";
  import CatalogState from "$lib/components/catalog/CatalogState.svelte";
  import UploadDropzone from "$lib/components/UploadDropzone.svelte";
  import type { UploadResponse } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { formatDuration } from "$lib/utils";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  let uploads = $state<UploadResponse[]>([]);
</script>

<svelte:head><title>{t('upload.title')} — SoundTime</title></svelte:head>

<div class="max-w-3xl space-y-7">
  <CatalogHeader title={t("upload.title")} />
  {#if !auth.isAuthenticated}
    <CatalogState message={t("shell.signInUpload")}>
      <Button href={signInLink(page.url)}>{t("nav.signIn")}</Button>
    </CatalogState>
  {:else}
    <UploadDropzone onuploaded={(result) => uploads = [result, ...uploads]} />
    {#if uploads.length}
      <section aria-live="polite">
        <h2 class="mb-3 text-xl font-semibold">{t("shell.uploaded")}</h2>
        {#each uploads as upload (upload)}
          <div class="flex items-center gap-3 border-b py-3">
            <CircleCheck size={18} class="text-primary" />
            <div class="min-w-0">
              <p class="truncate text-sm font-semibold">{upload.title}</p>
              <p class="text-xs text-muted-foreground">{upload.format} · {formatDuration(upload.duration)}</p>
            </div>
          </div>
        {/each}
      </section>
    {/if}
  {/if}
</div>
