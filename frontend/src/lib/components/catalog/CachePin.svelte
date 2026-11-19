<script lang="ts">
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { Button } from "$lib/components/ui/button";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { Pin, PinOff } from "lucide-svelte";

  let { scope, id }: { scope: "tracks" | "albums"; id: string } = $props();
  const auth = getAuthStore();
  const path = $derived(`/admin/cache/${scope}/${id}/pin`);
  const status = catalogQuery(() => auth.isAdmin ? api.get<{ eligible: boolean; pinned: boolean }>(path) : null);
  let busy = $state(false);
  let error = $state<"spaceError" | "pinError" | null>(null);

  async function toggle() {
    if (busy || !status.data) return;
    busy = true;
    error = null;
    try {
      if (status.data.pinned) await api.delete(path);
      else await api.put(path);
      status.retry();
    } catch (e) {
      error = e instanceof Error && "status" in e && e.status === 507 ? "spaceError" : "pinError";
    } finally {
      busy = false;
    }
  }
</script>

{#if auth.isAdmin && status.data?.eligible}
  <Button variant="outline" disabled={busy} onclick={toggle}>
    {#if status.data.pinned}<PinOff size={15} />{:else}<Pin size={15} />{/if}
    {t(busy ? "admin.cache.working" : status.data.pinned ? "admin.cache.unpin" : "admin.cache.pin")}
  </Button>
  {#if error}<span role="alert" class="text-sm text-destructive">{t(`admin.cache.${error}`)}</span>{/if}
{/if}
