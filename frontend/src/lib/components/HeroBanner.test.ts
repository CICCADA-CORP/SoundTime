import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => key,
}));

import HeroBanner from './HeroBanner.svelte';

describe('HeroBanner', () => {
  const editorialItem = {
    id: 'ep-1',
    name: 'Chill Vibes',
    description: 'Relax and unwind',
    cover_url: '/covers/chill.jpg',
    track_count: 15,
    tracks: [],
  };

  const albumItem = {
    id: 'album-1',
    title: 'Great Album',
    artist_id: 'a1',
    release_date: null,
    cover_url: null,
    genre: null,
    year: 2024,
    created_at: '2025-01-01',
    artist_name: 'Test Artist',
  };

  // ─── Editorial type tests ─────────────────────────────────────────

  it('renders editorial title using item.name', () => {
    render(HeroBanner, { props: { item: editorialItem } });
    expect(screen.getByText('Chill Vibes')).toBeInTheDocument();
  });

  it('renders "explore.featuredPlaylist" label for editorial type', () => {
    render(HeroBanner, { props: { item: editorialItem, type: 'editorial' } });
    expect(screen.getByText('explore.featuredPlaylist')).toBeInTheDocument();
  });

  it('renders cover image when cover_url is set (editorial)', () => {
    render(HeroBanner, { props: { item: editorialItem, type: 'editorial' } });
    const img = screen.getByAltText('Chill Vibes');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', '/covers/chill.jpg');
  });

  it('renders gradient div when cover_url is null (no img)', () => {
    const itemNoCover = { ...editorialItem, cover_url: null };
    const { container } = render(HeroBanner, { props: { item: itemNoCover, type: 'editorial' } });
    const gradientDiv = container.querySelector('.bg-gradient-to-br');
    expect(gradientDiv).toBeInTheDocument();
    expect(container.querySelector('img')).toBeNull();
  });

  it('renders description when provided', () => {
    render(HeroBanner, { props: { item: editorialItem, type: 'editorial' } });
    expect(screen.getByText('Relax and unwind')).toBeInTheDocument();
  });

  it('does not render description when null', () => {
    const itemNoDesc = { ...editorialItem, description: null };
    render(HeroBanner, { props: { item: itemNoDesc, type: 'editorial' } });
    expect(screen.queryByText('Relax and unwind')).not.toBeInTheDocument();
  });

  it('renders track count when present (editorial)', () => {
    render(HeroBanner, { props: { item: editorialItem, type: 'editorial' } });
    expect(screen.getByText('15 playlists.tracks')).toBeInTheDocument();
  });

  it('does not render track count when trackCount is 0 (falsy)', () => {
    const itemNoCount = { ...editorialItem, track_count: 0 };
    const { container } = render(HeroBanner, { props: { item: itemNoCount, type: 'editorial' } });
    expect(screen.queryByText('playlists.tracks')).not.toBeInTheDocument();
  });

  // ─── Album type tests ─────────────────────────────────────────────

  it('renders album title using item.title', () => {
    render(HeroBanner, { props: { item: albumItem, type: 'album' } });
    expect(screen.getByText('Great Album')).toBeInTheDocument();
  });

  it('renders "explore.featuredAlbum" label for album type', () => {
    render(HeroBanner, { props: { item: albumItem, type: 'album' } });
    expect(screen.getByText('explore.featuredAlbum')).toBeInTheDocument();
  });

  it('renders artist_name as description for album type', () => {
    render(HeroBanner, { props: { item: albumItem, type: 'album' } });
    expect(screen.getByText('Test Artist')).toBeInTheDocument();
  });

  it('does not render description when album has no artist_name', () => {
    const albumNoArtist = { ...albumItem, artist_name: undefined };
    const { container } = render(HeroBanner, { props: { item: albumNoArtist, type: 'album' } });
    // The description paragraph should not be present
    const paragraphs = container.querySelectorAll('p.text-sm');
    const descParagraphs = Array.from(paragraphs).filter(p => p.classList.contains('text-white/80'));
    expect(descParagraphs.length).toBe(0);
  });

  it('does not render trackCount for album type', () => {
    render(HeroBanner, { props: { item: albumItem, type: 'album' } });
    expect(screen.queryByText('playlists.tracks')).not.toBeInTheDocument();
  });

  it('renders gradient fallback when album has no cover_url', () => {
    const { container } = render(HeroBanner, { props: { item: albumItem, type: 'album' } });
    const gradientDiv = container.querySelector('.bg-gradient-to-br');
    expect(gradientDiv).toBeInTheDocument();
    expect(container.querySelector('img')).toBeNull();
  });

  it('renders cover image when album has cover_url', () => {
    const albumWithCover = { ...albumItem, cover_url: '/covers/album.jpg' };
    render(HeroBanner, { props: { item: albumWithCover, type: 'album' } });
    const img = screen.getByAltText('Great Album');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', '/covers/album.jpg');
  });

  // ─── onclick / default type ────────────────────────────────────────

  it('calls onclick when clicked', async () => {
    const handleClick = vi.fn();
    render(HeroBanner, { props: { item: editorialItem, onclick: handleClick } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });

  it('defaults to editorial type when type prop is not provided', () => {
    render(HeroBanner, { props: { item: editorialItem } });
    expect(screen.getByText('explore.featuredPlaylist')).toBeInTheDocument();
    expect(screen.getByText('Chill Vibes')).toBeInTheDocument();
  });
});
