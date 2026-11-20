<script lang="ts">
  import { X, LoaderCircle, CircleCheck, CircleAlert } from "lucide-svelte";
  import { getTaskStore } from "$lib/stores/tasks.svelte";
  import { t } from "$lib/i18n/index.svelte";

  const tasks = getTaskStore();
  const label = $derived(tasks.lastStatus?.status === "error" ? t("admin.taskBanner.error") :
    tasks.taskType === "sync"
      ? t(tasks.isRunning ? "admin.taskBanner.syncInProgress" : "admin.taskBanner.syncCompleted")
      : t(tasks.isRunning ? "admin.taskBanner.integrityInProgress" : "admin.taskBanner.integrityCompleted"));
</script>

{#if tasks.isActive}
  <div class="flex items-center justify-center gap-3 border-b bg-muted px-3 py-2 text-xs" role="status">
    {#if tasks.isRunning}<LoaderCircle size={14} class="animate-spin" />{:else if tasks.lastStatus?.status === "error"}<CircleAlert size={14} />{:else}<CircleCheck size={14} />{/if}
    <a href="/admin" class="hover:underline">{label}
      {#if tasks.progress} · {tasks.progress.processed}{tasks.progress.total ? ' / ' + tasks.progress.total : ''}{/if}
    </a>
    {#if !tasks.isRunning}<button onclick={tasks.dismiss} aria-label={t("common.dismiss")}><X size={15} /></button>{/if}
  </div>
{/if}
