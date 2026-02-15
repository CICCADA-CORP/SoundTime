<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { TosResponse } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";

  let content = $state("");
  let loading = $state(true);

  onMount(async () => {
    try {
      const tos = await api.get<TosResponse>("/tos");
      content = tos.content;
    } catch {
      content = t('tos.loadError');
    } finally {
      loading = false;
    }
  });

  function renderMarkdown(md: string): string {
    // Simple Markdown → HTML renderer for ToS content
    return md
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/^### (.+)$/gm, '<h3 class="text-lg font-semibold mt-6 mb-2">$1</h3>')
      .replace(/^## (.+)$/gm, '<h2 class="text-xl font-semibold mt-8 mb-3">$1</h2>')
      .replace(/^# (.+)$/gm, '<h1 class="text-2xl font-bold mt-8 mb-4">$1</h1>')
      .replace(/^\*\*(.+?)\*\*$/gm, '<p class="font-semibold">$1</p>')
      .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
      .replace(/\*(.+?)\*/g, "<em>$1</em>")
      .replace(/^- (.+)$/gm, '<li class="ml-4 list-disc">$1</li>')
      .replace(/^---$/gm, '<hr class="border-[hsl(var(--border))] my-6" />')
      .replace(/\n{2,}/g, "</p><p>")
      .replace(/^(?!<[hHlL])/gm, (match) => (match ? `<p>${match}` : ""))
      .replace(/<p><\/p>/g, "");
  }
</script>

<svelte:head>
  <title>{t('tos.title')}</title>
</svelte:head>

<div class="max-w-3xl mx-auto py-8 px-4">
  {#if loading}
    <div class="flex justify-center py-20">
      <div class="w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full animate-spin"></div>
    </div>
  {:else}
    <div class="prose prose-invert max-w-none space-y-2 text-[hsl(var(--foreground))] leading-relaxed text-sm">
      <!-- SECURITY: renderMarkdown() escapes &, <, > before applying transforms — safe from XSS -->
      {@html renderMarkdown(content)}
    </div>
  {/if}
</div>
