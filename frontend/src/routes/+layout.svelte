<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import AudioPlayer from "$lib/components/AudioPlayer.svelte";
  import SearchBar from "$lib/components/SearchBar.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { api } from "$lib/api";
  import type { SetupStatus } from "$lib/types";
  import { t } from "$lib/i18n/index.svelte";
  import {
    Home,
    Compass,
    Library,
    ListMusic,
    Heart,
    Clock,
    Upload,
    Settings,
    Disc3,
    LogIn,
    Shield,
  } from "lucide-svelte";
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();

  const auth = getAuthStore();
  const player = getPlayerStore();

  let setupChecked = $state(false);
  let instancePrivate = $state(false);

  onMount(async () => {
    try {
      const status = await api.get<SetupStatus>("/setup/status");
      if (!status.setup_complete && !$page.url.pathname.startsWith("/setup")) {
        goto("/setup");
        return;
      }
      instancePrivate = status.instance_private ?? false;
    } catch {
      // API not reachable
    }
    setupChecked = true;
    auth.init();
  });

  // Reactive guard: redirect to login if instance is private and user not authenticated
  $effect(() => {
    if (!setupChecked) return;
    if (!instancePrivate) return;
    const path = $page.url.pathname;
    const publicPaths = ["/login", "/register", "/setup"];
    if (publicPaths.some(p => path.startsWith(p))) return;
    if (!auth.isAuthenticated) {
      goto("/login");
    }
  });

  const navItems = [
    { href: "/", key: "nav.home" as const, icon: Home },
    { href: "/explore", key: "nav.explore" as const, icon: Compass },
    { href: "/library", key: "nav.library" as const, icon: Library },
  ];

  const libraryItems = [
    { href: "/playlists", key: "nav.playlists" as const, icon: ListMusic },
    { href: "/favorites", key: "nav.favorites" as const, icon: Heart },
    { href: "/history", key: "nav.history" as const, icon: Clock },
    { href: "/upload", key: "nav.upload" as const, icon: Upload },
  ];
</script>

{#if $page.url.pathname.startsWith("/setup")}
  <!-- Setup pages use their own minimal layout -->
  {@render children()}
{:else if !setupChecked}
  <div class="flex items-center justify-center h-screen bg-[hsl(var(--background))]">
    <div class="animate-spin w-8 h-8 border-2 border-[hsl(var(--primary))] border-t-transparent rounded-full"></div>
  </div>
{:else}
<div class="flex h-screen overflow-hidden">
  <!-- Sidebar -->
  <aside class="w-60 bg-[hsl(0,0%,5%)] flex flex-col border-r border-[hsl(var(--border))] flex-shrink-0" class:pb-20={player.currentTrack}>
    <!-- Logo -->
    <div class="p-5">
      <a href="/" class="flex items-center gap-2.5 text-lg font-semibold tracking-tight">
        <Disc3 class="w-6 h-6 text-[hsl(var(--primary))]" />
        <span>SoundTime</span>
      </a>
    </div>

    <!-- Nav -->
    <nav class="px-3 space-y-0.5">
      {#each navItems as item}
        {@const isActive = $page.url.pathname === item.href}
        <a
          href={item.href}
          class="flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors
            {isActive ? 'bg-[hsl(var(--secondary))] text-white' : 'text-[hsl(var(--muted-foreground))] hover:text-white hover:bg-white/5'}"
        >
          <item.icon class="w-[18px] h-[18px]" strokeWidth={1.75} />
          {t(item.key)}
        </a>
      {/each}
    </nav>

    <!-- Library Section -->
    {#if auth.isAuthenticated}
      <div class="mt-6 px-3">
        <h3 class="px-3 text-[11px] font-semibold text-[hsl(var(--muted-foreground))] uppercase tracking-widest mb-2">{t('nav.library')}</h3>
        <nav class="space-y-0.5">
          {#each libraryItems as item}
            {@const isActive = $page.url.pathname === item.href}
            <a
              href={item.href}
              class="flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors
                {isActive ? 'bg-[hsl(var(--secondary))] text-white' : 'text-[hsl(var(--muted-foreground))] hover:text-white hover:bg-white/5'}"
            >
              <item.icon class="w-[18px] h-[18px]" strokeWidth={1.75} />
              {t(item.key)}
            </a>
          {/each}
        </nav>
      </div>
    {/if}

    <!-- Bottom user -->
    <div class="mt-auto p-3 border-t border-[hsl(var(--border))]">
      {#if auth.isAuthenticated}
        <div class="flex items-center gap-2.5 px-3 py-2">
          <div class="w-8 h-8 rounded-full bg-[hsl(var(--secondary))] flex items-center justify-center text-sm font-medium">
            {auth.user?.username?.charAt(0).toUpperCase() ?? "?"}
          </div>
          <div class="min-w-0 flex-1">
            <p class="text-sm font-medium truncate">{auth.user?.username}</p>
          </div>
          <a href="/settings" class="text-[hsl(var(--muted-foreground))] hover:text-white transition-colors" aria-label="Settings">
            <Settings class="w-4 h-4" strokeWidth={1.75} />
          </a>
        </div>
      {:else}
        <a href="/login" class="flex items-center justify-center gap-2 w-full px-4 py-2 bg-[hsl(var(--primary))] text-white rounded-md text-sm font-medium hover:opacity-90 transition">
          <LogIn class="w-4 h-4" strokeWidth={1.75} />
          {t('nav.signIn')}
        </a>
      {/if}
    </div>
  </aside>

  <!-- Main Content -->
  <main class="flex-1 overflow-y-auto" class:pb-20={player.currentTrack}>
    <!-- Top Bar -->
    <header class="sticky top-0 z-40 bg-[hsl(var(--background))]/80 backdrop-blur-lg border-b border-[hsl(var(--border))]">
      <div class="flex items-center justify-between px-6 py-3">
        <SearchBar />
        {#if auth.isAuthenticated && auth.isAdmin}
          <a href="/admin" class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-full bg-[hsl(var(--primary))]/20 text-[hsl(var(--primary))] hover:bg-[hsl(var(--primary))]/30 transition-colors font-medium">
            <Shield class="w-3.5 h-3.5" strokeWidth={1.75} />
            Admin
          </a>
        {/if}
      </div>
    </header>

    <div class="p-6">
      {@render children()}
    </div>
  </main>
</div>

{/if}

<!-- Audio Player Bar (always mounted to prevent music interruption on navigation) -->
<AudioPlayer />
