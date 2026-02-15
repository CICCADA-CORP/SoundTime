import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const mockPlayQueue = vi.fn();
vi.mock('$lib/stores/queue.svelte', () => ({
  getQueueStore: () => ({
    playQueue: mockPlayQueue,
    queue: [],
    currentIndex: -1,
    currentTrack: null,
    hasNext: false,
    hasPrevious: false,
    addToQueue: vi.fn(),
    addNext: vi.fn(),
    removeFromQueue: vi.fn(),
    clearQueue: vi.fn(),
    next: vi.fn(),
    previous: vi.fn(),
  }),
}));

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

  beforeEach(() => {
    vi.clearAllMocks();
  });

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
    const albumNoArtist = { ...album, artist_name: undefined };
    render(AlbumCard, { props: { album: albumNoArtist } });
    // Should still render
    expect(screen.getByText('Test Album')).toBeInTheDocument();
  });

  it('renders empty when both year and artist_name are null', () => {
    const albumMinimal = { ...album, year: null, artist_name: undefined };
    render(AlbumCard, { props: { album: albumMinimal } });
    expect(screen.getByText('Test Album')).toBeInTheDocument();
  });

  it('calls playQueue when play button clicked with tracks', async () => {
    const trackData = [{
      id: 't1', title: 'Track 1', artist_id: 'a1', album_id: 'album-1',
      track_number: 1, disc_number: null, duration_secs: 200, genre: null,
      year: null, file_path: '/t1.mp3', file_size: 1000, format: 'mp3',
      bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 0, created_at: '2025-01-01',
    }];
    const albumWithTracks = { ...album, tracks: trackData };
    render(AlbumCard, { props: { album: albumWithTracks } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);
    expect(mockPlayQueue).toHaveBeenCalledWith(trackData);
  });

  it('does not call playQueue when album has no tracks', async () => {
    const albumNoTracks = { ...album, tracks: undefined };
    render(AlbumCard, { props: { album: albumNoTracks } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);
    expect(mockPlayQueue).not.toHaveBeenCalled();
  });

  it('does not call playQueue when album has empty tracks array', async () => {
    const albumEmptyTracks = { ...album, tracks: [] as any[] };
    render(AlbumCard, { props: { album: albumEmptyTracks } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);
    expect(mockPlayQueue).not.toHaveBeenCalled();
  });
});
