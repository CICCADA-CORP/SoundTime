<script lang="ts">
  import { goto } from "$app/navigation";
  import { Dialog } from "bits-ui";
  import { Plus } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { Button } from "$lib/components/ui/button";
  import PlaylistForm from "./PlaylistForm.svelte";

  let open = $state(false);
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger>
    {#snippet child({ props })}
      <Button {...props}><Plus size={16} />{t("playlists.createNew")}</Button>
    {/snippet}
  </Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-[150] bg-black/40" />
    <Dialog.Content class="fixed left-1/2 top-1/2 z-[151] max-h-[90dvh] w-[min(28rem,calc(100%-2rem))] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-xl bg-background p-6 shadow-xl">
      <Dialog.Title class="mb-5 text-xl font-semibold">{t("playlists.createNew")}</Dialog.Title>
      <PlaylistForm oncancel={() => open = false} onsave={playlist => { open = false; void goto(`/playlists/${playlist.id}`); }} />
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
