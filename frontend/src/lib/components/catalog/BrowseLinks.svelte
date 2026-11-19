<script lang="ts">
  import { ChevronRight, Disc3, ListMusic, Mic2 } from "lucide-svelte";
  import { t } from "$lib/i18n/index.svelte";

  const destinations = [
    { href: "/albums", label: "explore.albums", icon: Disc3 },
    { href: "/artists", label: "explore.artists", icon: Mic2 },
    { href: "/playlists", label: "nav.playlists", icon: ListMusic },
  ] as const;
</script>

<nav aria-label={t("nav.explore")} class="grid grid-cols-1 gap-3 sm:grid-cols-3">
  {#each destinations as item (item.href)}
    <a class="flex min-h-14 items-center gap-3 rounded-lg border border-border px-4 text-sm font-medium hover:bg-muted" href={item.href}>
      <item.icon size={20} strokeWidth={1.6} class="text-muted-foreground" />
      {t(item.label)}
      <ChevronRight size={16} class="ml-auto text-muted-foreground" />
    </a>
  {/each}
</nav>
