import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ArtistCard from './ArtistCard.svelte';

describe('ArtistCard', () => {
  const artist = {
    id: 'artist-1',
    name: 'Cool Artist',
    bio: null,
    image_url: null,
    created_at: '2025-01-01',
  };

  it('renders artist name', () => {
    render(ArtistCard, { props: { artist } });
    expect(screen.getByText('Cool Artist')).toBeInTheDocument();
  });

  it('renders fallback emoji when no image_url', () => {
    render(ArtistCard, { props: { artist } });
    expect(screen.getByText('🎤')).toBeInTheDocument();
  });

  it('renders image when image_url is set', () => {
    const artistWithImage = { ...artist, image_url: '/img/artist.jpg' };
    render(ArtistCard, { props: { artist: artistWithImage } });
    const img = screen.getByAltText('Cool Artist');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', '/img/artist.jpg');
  });

  it('links to artist page', () => {
    render(ArtistCard, { props: { artist } });
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', '/artists/artist-1');
  });

  it('shows Artist label', () => {
    render(ArtistCard, { props: { artist } });
    expect(screen.getByText('Artist')).toBeInTheDocument();
  });
});
