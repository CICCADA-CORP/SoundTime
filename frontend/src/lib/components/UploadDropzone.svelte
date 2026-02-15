<script lang="ts">
  import { api } from "$lib/api";
  import type { UploadResponse } from "$lib/types";
  import { Upload, X, CheckCircle, AlertCircle, Music } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";

  let { onuploaded }: { onuploaded?: (result: UploadResponse) => void } = $props();

  interface FileQueueItem {
    file: File;
    status: "pending" | "uploading" | "done" | "error";
    progress: number;
    result?: UploadResponse;
    error?: string;
    abortFn?: () => void;
  }

  let isDragging = $state(false);
  let queue = $state<FileQueueItem[]>([]);
  let fileInput: HTMLInputElement;

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave() {
    isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    const files = e.dataTransfer?.files;
    if (files) addFiles(files);
  }

  function handleFileSelect(e: Event) {
    const target = e.target as HTMLInputElement;
    if (target.files) {
      addFiles(target.files);
      target.value = "";
    }
  }

  function addFiles(fileList: FileList) {
    const newItems: FileQueueItem[] = [];
    for (const file of Array.from(fileList)) {
      // Deduplicate by name+size
      const exists = queue.some(
        (q) => q.file.name === file.name && q.file.size === file.size
      );
      if (!exists) {
        newItems.push({ file, status: "pending", progress: 0 });
      }
    }
    queue = [...queue, ...newItems];
    processQueue();
  }

  function removeFromQueue(index: number) {
    const item = queue[index];
    if (item.abortFn && item.status === "uploading") {
      item.abortFn();
    }
    queue = queue.filter((_, i) => i !== index);
  }

  async function processQueue() {
    // Upload one at a time to avoid overloading
    const next = queue.find((q) => q.status === "pending");
    if (!next) return;
    const isUploading = queue.some((q) => q.status === "uploading");
    if (isUploading) return;

    next.status = "uploading";
    queue = [...queue];

    try {
      const formData = new FormData();
      formData.append("file", next.file);

      const { promise, abort } = api.uploadWithProgress<UploadResponse>(
        "/upload",
        formData,
        (loaded, total) => {
          next.progress = Math.round((loaded / total) * 100);
          queue = [...queue];
        }
      );

      next.abortFn = abort;
      queue = [...queue];

      const result = await promise;
      next.status = "done";
      next.progress = 100;
      next.result = result;
      queue = [...queue];
      onuploaded?.(result);
    } catch (e) {
      next.status = "error";
      next.error = e instanceof Error ? e.message : "Upload failed";
      queue = [...queue];
    }

    // Process next file
    processQueue();
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return t('upload.sizeB', { n: bytes });
    if (bytes < 1024 * 1024) return t('upload.sizeKB', { n: (bytes / 1024).toFixed(1) });
    return t('upload.sizeMB', { n: (bytes / (1024 * 1024)).toFixed(1) });
  }

  let uploading = $derived(queue.some((q) => q.status === "uploading"));
  let pendingCount = $derived(queue.filter((q) => q.status === "pending").length);
  let doneCount = $derived(queue.filter((q) => q.status === "done").length);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="border-2 border-dashed rounded-xl p-8 text-center transition cursor-pointer"
  class:border-[hsl(142,71%,45%)]={isDragging}
  class:bg-green-500={isDragging}
  class:bg-opacity-5={isDragging}
  class:border-[hsl(var(--border))]={!isDragging}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
  onclick={() => fileInput.click()}
  role="button"
  tabindex="0"
  onkeydown={(e) => e.key === 'Enter' && fileInput.click()}
>
  <input
    bind:this={fileInput}
    type="file"
    accept="audio/*,.aif,.aiff"
    multiple
    class="hidden"
    onchange={handleFileSelect}
  />

  <div class="flex flex-col items-center gap-3">
    <Upload class="w-12 h-12 text-[hsl(var(--muted-foreground))]" />
    <p class="text-sm font-medium">{t('upload.dropHint')}</p>
    <p class="text-xs text-[hsl(var(--muted-foreground))]">{t('upload.supportedFormats')}</p>
  </div>
</div>

<!-- Upload Queue -->
{#if queue.length > 0}
  <div class="mt-4 space-y-2">
    <div class="flex items-center justify-between text-xs text-[hsl(var(--muted-foreground))]">
      <span>{t('upload.queueProgress', { done: doneCount, total: queue.length })}{pendingCount > 0 ? ` · ${t('upload.queuePending', { count: pendingCount })}` : ''}</span>
      {#if !uploading && queue.length > 0}
        <button
          class="text-xs underline hover:text-[hsl(var(--foreground))]"
          onclick={(e) => { e.stopPropagation(); queue = []; }}
        >
           {t('upload.clearAll')}
        </button>
      {/if}
    </div>

    {#each queue as item, i (item.file.name + item.file.size)}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="flex items-center gap-3 p-3 bg-[hsl(var(--card))] rounded-lg" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="listitem">
        <!-- Icon -->
        <div class="shrink-0">
          {#if item.status === "done"}
            <CheckCircle class="w-5 h-5 text-green-400" />
          {:else if item.status === "error"}
            <AlertCircle class="w-5 h-5 text-red-400" />
          {:else}
            <Music class="w-5 h-5 text-[hsl(var(--muted-foreground))]" />
          {/if}
        </div>

        <!-- File info + progress -->
        <div class="flex-1 min-w-0">
          <div class="flex items-center justify-between">
            <p class="text-sm font-medium truncate">{item.file.name}</p>
            <span class="text-xs text-[hsl(var(--muted-foreground))] shrink-0 ml-2">{formatSize(item.file.size)}</span>
          </div>

          {#if item.status === "uploading"}
            <div class="mt-1.5 w-full bg-[hsl(var(--secondary))] rounded-full h-1.5 overflow-hidden">
              <div
                class="h-full bg-[hsl(var(--primary))] rounded-full transition-all duration-200"
                style="width: {item.progress}%"
              ></div>
            </div>
            <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">{item.progress}%</p>
          {:else if item.status === "done" && item.result}
            <p class="text-xs text-green-400 mt-0.5">{item.result.title} · {item.result.format} · {Math.round(item.result.duration)}s</p>
          {:else if item.status === "error"}
            <p class="text-xs text-red-400 mt-0.5">{item.error}</p>
          {:else}
            <p class="text-xs text-[hsl(var(--muted-foreground))] mt-0.5">{t('upload.pending')}</p>
          {/if}
        </div>

        <!-- Remove button -->
        <button
          class="shrink-0 p-1 rounded-md hover:bg-[hsl(var(--secondary))] text-[hsl(var(--muted-foreground))] hover:text-[hsl(var(--foreground))]"
          onclick={(e) => { e.stopPropagation(); removeFromQueue(i); }}
           title={t('upload.remove')}
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    {/each}
  </div>
{/if}
