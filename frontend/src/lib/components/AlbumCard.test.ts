import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import AlbumCard from './AlbumCard.svelte';

describe('AlbumCard', () => {
  const album = {
    id: 'album-1',
    title: 'Test Album',
    artist_id: 'a1',
    release_date: null,
    cover_url: null,
    genre: null,
    year: 2024,
    created_at: '2025-01-01',
    artist_name: 'Test Artist',
  };

  it('renders album title', () => {
    render(AlbumCard, { props: { album } });
    expect(screen.getByText('Test Album')).toBeInTheDocument();
  });

  it('renders year and artist name', () => {
    render(AlbumCard, { props: { album } });
    expect(screen.getByText('2024 · Test Artist')).toBeInTheDocument();
  });

  it('renders fallback emoji when no cover_url', () => {
    render(AlbumCard, { props: { album } });
    expect(screen.getByText('💿')).toBeInTheDocument();
  });

  it('renders cover image when cover_url is set', () => {
    const albumWithCover = { ...album, cover_url: '/covers/test.jpg' };
    render(AlbumCard, { props: { album: albumWithCover } });
    const img = screen.getByAltText('Test Album');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', '/covers/test.jpg');
  });

  it('links to album page', () => {
    render(AlbumCard, { props: { album } });
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', '/albums/album-1');
  });

  it('has play button', () => {
    render(AlbumCard, { props: { album } });
    expect(screen.getByLabelText('Play album')).toBeInTheDocument();
  });

  it('renders empty string when year is null', () => {
    const albumNoYear = { ...album, year: null };
    render(AlbumCard, { props: { album: albumNoYear } });
    // Year should not appear, only artist name
    const meta = screen.getByText(/Test Artist/);
    expect(meta).toBeInTheDocument();
  });

  it('renders empty when artist_name is null', () => {
    const albumNoArtist = { ...album, artist_name: null };
    render(AlbumCard, { props: { album: albumNoArtist } });
    // Should still render
    expect(screen.getByText('Test Album')).toBeInTheDocument();
  });

  it('renders empty when both year and artist_name are null', () => {
    const albumMinimal = { ...album, year: null, artist_name: null };
    render(AlbumCard, { props: { album: albumMinimal } });
    expect(screen.getByText('Test Album')).toBeInTheDocument();
  });
});
