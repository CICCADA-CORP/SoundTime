import {
  Compass, Disc3, Heart, History, House, LibraryBig, ListMusic,
  Antenna, MicVocal, Music2, Radio, Settings2, Shield, Upload,
} from "lucide-svelte";
import type { TranslationKey } from "$lib/i18n/index.svelte";

export interface NavItem {
  href: string;
  label: TranslationKey;
  icon: typeof Radio;
}

export const home: NavItem = { href: "/", label: "nav.home", icon: House };
export const listening: NavItem[] = [
  { href: "/explore", label: "nav.explore", icon: Compass },
  { href: "/radio", label: "nav.radio", icon: Radio },
  { href: "/webradios", label: "nav.webradios", icon: Antenna },
];
export const discover: NavItem[] = [home, ...listening];

export const libraryHome: NavItem = { href: "/library", label: "shell.collection", icon: LibraryBig };
export const catalog: NavItem[] = [
  { href: "/tracks", label: "explore.tracks", icon: Music2 },
  { href: "/albums", label: "library.albums", icon: Disc3 },
  { href: "/artists", label: "shell.artists", icon: MicVocal },
  { href: "/playlists", label: "nav.playlists", icon: ListMusic },
];

export const personal: NavItem[] = [
  { href: "/favorites", label: "nav.favorites", icon: Heart },
  { href: "/history", label: "nav.history", icon: History },
];

export const upload: NavItem = { href: "/upload", label: "nav.upload", icon: Upload };
export const settings: NavItem = { href: "/settings", label: "nav.settings", icon: Settings2 };
export const utilities: NavItem[] = [upload, settings];
export const admin: NavItem = { href: "/admin", label: "nav.admin", icon: Shield };

export function isActive(path: string, href: string) {
  return path === href || (href !== "/" && path.startsWith(href + "/"));
}

export function followSidebarLink(event: MouseEvent, close?: () => void) {
  if (!event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey) close?.();
}
