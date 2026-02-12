<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import AudioPlayer from "$lib/components/AudioPlayer.svelte";
  import SearchBar from "$lib/components/SearchBar.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { getPlayerStore } from "$lib/stores/player.svelte";
  import { getThemeStore } from "$lib/stores/theme.svelte";
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
    Menu,
    X,
    Search,
  } from "lucide-svelte";
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();

  const auth = getAuthStore();
  const player = getPlayerStore();
  const theme = getThemeStore();

  let setupChecked = $state(false);
  let instancePrivate = $state(false);
  let sidebarOpen = $state(false);
  let mobileSearchOpen = $state(false);

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
    theme.init();
  });

  // Close sidebar on navigation
  $effect(() => {
    void $page.url.pathname;
    sidebarOpen = false;
    mobileSearchOpen = false;
  });

  // Reactively inject/remove theme stylesheet
  $effect(() => {
    if (theme.activeTheme) {
      theme.injectTheme();
    } else {
      theme.removeTheme();
    }
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

  // Bottom nav items for mobile
  const mobileNavItems = [
    { href: "/", key: "nav.home" as const, icon: Home },
    { href: "/explore", key: "nav.explore" as const, icon: Compass },
    { href: "/library", key: "nav.library" as const, icon: Library },
    { href: "/favorites", key: "nav.favorites" as const, icon: Heart },
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
  <!-- Mobile sidebar overlay -->
  {#if sidebarOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 bg-black/60 z-[60] md:hidden"
      onclick={() => sidebarOpen = false}
      onkeydown={(e) => e.key === 'Escape' && (sidebarOpen = false)}
    ></div>
  {/if}

  <!-- Sidebar -->
  <aside
    class="fixed md:static inset-y-0 left-0 z-[70] w-60 bg-[hsl(0,0%,5%)] flex flex-col border-r border-[hsl(var(--border))] flex-shrink-0 transform transition-transform duration-200 ease-in-out
      {sidebarOpen ? 'translate-x-0' : '-translate-x-full'} md:translate-x-0"
    class:pb-20={player.currentTrack}
  >
    <!-- Logo + close button -->
    <div class="p-5 flex items-center justify-between">
      <a href="/" class="flex items-center gap-2.5 text-lg font-semibold tracking-tight">
        <Disc3 class="w-6 h-6 text-[hsl(var(--primary))]" />
        <span>SoundTime</span>
      </a>
      <button class="md:hidden text-[hsl(var(--muted-foreground))] hover:text-white transition-colors p-1" onclick={() => sidebarOpen = false} aria-label="Close menu">
        <X class="w-5 h-5" strokeWidth={1.75} />
      </button>
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
  <main class="flex-1 overflow-y-auto w-full min-w-0 {player.currentTrack ? 'pb-40 md:pb-24' : 'pb-20 md:pb-0'}">
    <!-- Top Bar -->
    <header class="sticky top-0 z-40 bg-[hsl(var(--background))]/80 backdrop-blur-lg border-b border-[hsl(var(--border))]">
      <div class="flex items-center justify-between px-3 md:px-6 py-3 gap-2">
        <!-- Mobile hamburger -->
        <button class="md:hidden text-[hsl(var(--muted-foreground))] hover:text-white transition-colors p-1.5 -ml-1" onclick={() => sidebarOpen = true} aria-label="Open menu">
          <Menu class="w-5 h-5" strokeWidth={1.75} />
        </button>

        <!-- Logo on mobile (centered feel) -->
        <a href="/" class="md:hidden flex items-center gap-1.5 text-base font-semibold tracking-tight">
          <Disc3 class="w-5 h-5 text-[hsl(var(--primary))]" />
          <span>SoundTime</span>
        </a>

        <!-- Search (desktop) -->
        <div class="hidden md:block flex-1">
          <SearchBar />
        </div>

        <!-- Mobile search toggle + admin -->
        <div class="flex items-center gap-1.5">
          <button class="md:hidden text-[hsl(var(--muted-foreground))] hover:text-white transition-colors p-1.5" onclick={() => mobileSearchOpen = !mobileSearchOpen} aria-label="Search">
            <Search class="w-5 h-5" strokeWidth={1.75} />
          </button>
          {#if auth.isAuthenticated && auth.isAdmin}
            <a href="/admin" class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-full bg-[hsl(var(--primary))]/20 text-[hsl(var(--primary))] hover:bg-[hsl(var(--primary))]/30 transition-colors font-medium">
              <Shield class="w-3.5 h-3.5" strokeWidth={1.75} />
              <span class="hidden sm:inline">Admin</span>
            </a>
          {/if}
        </div>
      </div>

      <!-- Mobile search bar (expandable) -->
      {#if mobileSearchOpen}
        <div class="md:hidden px-3 pb-3">
          <SearchBar />
        </div>
      {/if}
    </header>

    <div class="p-3 md:p-6">
      {@render children()}
    </div>
  </main>
</div>

<!-- Mobile bottom navigation -->
<nav class="fixed bottom-0 left-0 right-0 z-[55] md:hidden bg-[hsl(0,0%,5%)] border-t border-[hsl(var(--border))] safe-area-bottom"
  class:hidden={!setupChecked || $page.url.pathname.startsWith("/setup")}
>
  <div class="flex items-center justify-around h-14">
    {#each mobileNavItems as item}
      {@const isActive = $page.url.pathname === item.href}
      <a
        href={item.href}
        class="flex flex-col items-center gap-0.5 px-3 py-1.5 rounded-lg transition-colors min-w-[60px]
          {isActive ? 'text-[hsl(var(--primary))]' : 'text-[hsl(var(--muted-foreground))]'}"
      >
        <item.icon class="w-5 h-5" strokeWidth={isActive ? 2.25 : 1.75} />
        <span class="text-[10px] font-medium">{t(item.key)}</span>
      </a>
    {/each}
    {#if auth.isAuthenticated}
      <button
        class="flex flex-col items-center gap-0.5 px-3 py-1.5 rounded-lg transition-colors min-w-[60px] text-[hsl(var(--muted-foreground))]"
        onclick={() => sidebarOpen = true}
      >
        <Menu class="w-5 h-5" strokeWidth={1.75} />
        <span class="text-[10px] font-medium">{t('nav.more')}</span>
      </button>
    {:else}
      <a
        href="/login"
        class="flex flex-col items-center gap-0.5 px-3 py-1.5 rounded-lg transition-colors min-w-[60px] text-[hsl(var(--muted-foreground))]"
      >
        <LogIn class="w-5 h-5" strokeWidth={1.75} />
        <span class="text-[10px] font-medium">{t('nav.signIn')}</span>
      </a>
    {/if}
  </div>
</nav>

{/if}

<!-- Audio Player Bar (always mounted to prevent music interruption on navigation) -->
<AudioPlayer />
