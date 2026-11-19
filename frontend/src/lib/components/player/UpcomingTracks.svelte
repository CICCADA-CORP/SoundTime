<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import { ArrowDown, ArrowUp, Disc3, Ellipsis, Play, X } from "lucide-svelte";
  import { playerArtwork } from "$lib/components/player/artwork";
  import { t } from "$lib/i18n/index.svelte";
  import type { Track } from "$lib/types";
  import { formatDuration } from "$lib/utils";

  let { tracks, onplay, onmove, onremove }: {
    tracks: Track[];
    onplay: (offset: number) => void;
    onmove: (offset: number, direction: -1 | 1) => void;
    onremove: (offset: number) => void;
  } = $props();

  const entries = $derived.by(() => {
    // An album or playlist can place the same recording in the queue more than once.
    const occurrences: Record<string, number> = Object.create(null);
    return tracks.map(track => {
      const occurrence = occurrences[track.id] ?? 0;
      occurrences[track.id] = occurrence + 1;
      return { key: `${track.id}:${occurrence}`, track, cover: playerArtwork(track) };
    });
  });

  const menuItem = "flex min-h-9 cursor-default select-none items-center gap-2.5 rounded-lg px-2.5 text-sm outline-none data-[highlighted]:bg-muted data-[disabled]:pointer-events-none data-[disabled]:opacity-40";
</script>

<ol class="divide-y divide-border/65">
  {#each entries as { key, track, cover }, index (key)}
    <li class="flex min-w-0 items-center gap-1 py-1.5">
      <button
        type="button"
        class="group flex min-w-0 flex-1 items-center gap-3 rounded-lg py-1 text-left"
        onclick={() => onplay(index)}
      >
        <span class="relative grid size-[52px] shrink-0 place-items-center overflow-hidden rounded-lg bg-muted">
          {#if cover}
            <img src={cover} alt="" loading="lazy" class="size-full object-cover" />
          {:else}
            <Disc3 size={22} class="text-muted-foreground" />
          {/if}
          <span class="absolute inset-0 grid place-items-center bg-black/40 text-white opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100">
            <Play size={17} fill="currentColor" />
          </span>
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm font-semibold leading-5 group-hover:text-primary">{track.title}</span>
          {#if track.artist_name}
            <span class="block truncate text-xs leading-5 text-muted-foreground">{track.artist_name}</span>
          {/if}
        </span>
        <span class="pl-1 text-xs tabular-nums text-muted-foreground">{formatDuration(track.duration_secs)}</span>
      </button>

      <DropdownMenu.Root>
        <DropdownMenu.Trigger
          aria-label={t("track.options") + " · " + track.title}
          class="grid size-10 shrink-0 place-items-center rounded-full text-muted-foreground hover:bg-muted hover:text-primary data-[state=open]:bg-muted"
        >
          <Ellipsis size={18} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" sideOffset={3} loop class="z-10 min-w-36 rounded-xl border border-border bg-popover p-1 text-popover-foreground shadow-lg">
          <DropdownMenu.Item class={menuItem} disabled={index === 0} onSelect={() => onmove(index, -1)}>
            <ArrowUp size={15} />{t("shell.moveUp")}
          </DropdownMenu.Item>
          <DropdownMenu.Item class={menuItem} disabled={index === entries.length - 1} onSelect={() => onmove(index, 1)}>
            <ArrowDown size={15} />{t("shell.moveDown")}
          </DropdownMenu.Item>
          <DropdownMenu.Separator class="my-1 h-px bg-border" />
          <DropdownMenu.Item class={menuItem} onSelect={() => onremove(index)}>
            <X size={15} />{t("upload.remove")}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </li>
  {/each}
</ol>
