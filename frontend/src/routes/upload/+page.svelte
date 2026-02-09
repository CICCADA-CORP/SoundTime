<script lang="ts">
  import UploadDropzone from "$lib/components/UploadDropzone.svelte";
  import type { UploadResponse } from "$lib/types";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const auth = getAuthStore();
  let uploads: UploadResponse[] = [];

  function handleUploaded(result: UploadResponse) {
    uploads = [result, ...uploads];
  }
</script>

<svelte:head><title>Upload — SoundTime</title></svelte:head>

<div class="max-w-2xl mx-auto space-y-6">
  <h1 class="text-2xl font-bold">{t('upload.title')}</h1>

  {#if !auth.isAuthenticated}
    <div class="text-center py-16">
      <p class="text-[hsl(var(--muted-foreground))] mb-4">Sign in to upload music.</p>
      <a href="/login" class="inline-block px-6 py-2 bg-[hsl(var(--primary))] text-white rounded-full text-sm font-medium">Sign In</a>
    </div>
  {:else}
    <UploadDropzone onuploaded={handleUploaded} />

    {#if uploads.length > 0}
      <div class="space-y-3">
        <h2 class="text-lg font-semibold">Uploaded</h2>
        {#each uploads as upload}
          <div class="flex items-center gap-3 p-3 bg-[hsl(var(--card))] rounded-lg">
            <span class="text-xl">✅</span>
            <div class="flex-1">
              <p class="text-sm font-medium">{upload.title}</p>
              <p class="text-xs text-[hsl(var(--muted-foreground))]">{upload.format} · {Math.round(upload.duration)}s</p>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
