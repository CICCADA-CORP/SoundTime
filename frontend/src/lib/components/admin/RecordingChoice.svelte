<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import { formatDuration } from "$lib/utils";
  import type { MetadataRecordingCandidate } from "$lib/types";

  let { title, candidates, busy, choose }: {
    title: string;
    candidates: MetadataRecordingCandidate[];
    busy: boolean;
    choose: (recordingId: string) => void;
  } = $props();
  const groupId = $props.id();
  let selected = $state("");
  const options = $derived(candidates.slice(0, 5));
  const canSubmit = $derived(!busy && options.some(candidate => candidate.recording_mbid === selected));
</script>

<form onsubmit={(event) => {
  event.preventDefault();
  if (canSubmit) choose(selected);
}}>
  <fieldset disabled={busy} class="space-y-2">
    <legend class="mb-2 font-medium">{t("admin.metadata.chooseRecording", { title })}</legend>
    {#each options as candidate (candidate.recording_mbid)}
      <label class="flex items-start gap-2 rounded border border-[hsl(var(--border))] px-3 py-2">
        <input type="radio" name={groupId} bind:group={selected} value={candidate.recording_mbid} class="mt-1" />
        <span class="min-w-0">
          <span class="block font-medium">{candidate.title}</span>
          <span class="block text-xs text-[hsl(var(--muted-foreground))]">
            {[candidate.artist_name, candidate.album_title,
              candidate.duration_ms == null ? null : formatDuration(candidate.duration_ms / 1000),
              candidate.disambiguation].filter(Boolean).join(" · ")}
          </span>
          <span class="block break-all font-mono text-xs text-[hsl(var(--muted-foreground))]">{candidate.recording_mbid}</span>
        </span>
      </label>
    {/each}
    <button type="submit" disabled={!canSubmit}
      class="rounded bg-[hsl(var(--primary))] px-3 py-2 text-sm font-medium text-[hsl(var(--primary-foreground))] disabled:opacity-50">
      {t("admin.metadata.applyRecording")}
    </button>
  </fieldset>
</form>
