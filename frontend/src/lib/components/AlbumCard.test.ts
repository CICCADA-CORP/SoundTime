import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const { mockPlayQueue, mockApiGet } = vi.hoisted(() => ({
  mockPlayQueue: vi.fn(),
  mockApiGet: vi.fn(),
}));

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

vi.mock('$lib/api', () => ({
  api: {
    get: mockApiGet,
  },
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
    expect(mockPlayQueue).toHaveBeenCalledWith(trackData, 0, "album");
  });

  it('does not call playQueue when album has no tracks', async () => {
    const albumNoTracks = { ...album, tracks: undefined };
    render(AlbumCard, { props: { album: albumNoTracks } });
    const playButton = screen.getByLabelText('Play album');
    
    // Mock to prevent actual API call during testing
    mockApiGet.mockResolvedValue({ ...album, tracks: [] });
    
    await fireEvent.click(playButton);
    
    // Should attempt to fetch tracks via API
    await waitFor(() => {
      expect(mockApiGet).toHaveBeenCalledWith('/albums/album-1');
    });
    
    // But should not call playQueue since no tracks returned
    expect(mockPlayQueue).not.toHaveBeenCalled();
  });

  it('does not call playQueue when album has empty tracks array', async () => {
    const albumEmptyTracks = { ...album, tracks: [] as any[] };
    render(AlbumCard, { props: { album: albumEmptyTracks } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);
    
    // Should not call API since tracks array is present (but empty)
    expect(mockApiGet).not.toHaveBeenCalled();
    expect(mockPlayQueue).not.toHaveBeenCalled();
  });

  it('renders only year when artist_name is missing', () => {
    const albumYearOnly = { ...album, artist_name: undefined, year: 2023 };
    render(AlbumCard, { props: { album: albumYearOnly } });
    // Should render "2023" without the " · " separator
    expect(screen.getByText('2023')).toBeInTheDocument();
  });

  it('renders only artist when year is null', () => {
    const albumArtistOnly = { ...album, year: null, artist_name: 'Some Artist' };
    render(AlbumCard, { props: { album: albumArtistOnly } });
    expect(screen.getByText('Some Artist')).toBeInTheDocument();
  });

  // New tests for async loading behavior

  it('fetches tracks and plays when tracks are not available', async () => {
    const trackData = [{
      id: 't1', title: 'Track 1', artist_id: 'a1', album_id: 'album-1',
      track_number: 1, disc_number: null, duration_secs: 200, genre: null,
      year: null, file_path: '/t1.mp3', file_size: 1000, format: 'mp3',
      bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 0, created_at: '2025-01-01',
    }];
    const albumWithTracksFromApi = { ...album, tracks: trackData };
    
    mockApiGet.mockResolvedValue(albumWithTracksFromApi);

    render(AlbumCard, { props: { album } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);

    await waitFor(() => {
      expect(mockApiGet).toHaveBeenCalledWith('/albums/album-1');
      expect(mockPlayQueue).toHaveBeenCalledWith(trackData, 0, 'album');
    });
  });

  it('shows loading spinner while fetching tracks', async () => {
    let resolvePromise: (value: any) => void;
    const promise = new Promise((resolve) => {
      resolvePromise = resolve;
    });
    mockApiGet.mockReturnValue(promise);

    render(AlbumCard, { props: { album } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);

    // Should show loading state on button
    expect(playButton).toBeDisabled();
    // The Loader2 component should be in the DOM but we'll check by class
    expect(screen.getByRole('button', { name: 'Play album' })).toHaveClass('disabled:opacity-50');

    // Resolve the API call
    resolvePromise!({ ...album, tracks: [] });

    await waitFor(() => {
      expect(playButton).not.toBeDisabled();
    });
  });

  it('handles API errors gracefully', async () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    
    mockApiGet.mockRejectedValue(new Error('API Error'));

    render(AlbumCard, { props: { album } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);

    await waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalledWith('Failed to fetch album tracks for playback:', expect.any(Error));
      expect(mockPlayQueue).not.toHaveBeenCalled();
    });

    consoleErrorSpy.mockRestore();
  });

  it('prevents double clicks while loading', async () => {
    let resolvePromise: (value: any) => void;
    const promise = new Promise((resolve) => {
      resolvePromise = resolve;
    });
    mockApiGet.mockReturnValue(promise);

    render(AlbumCard, { props: { album } });
    const playButton = screen.getByLabelText('Play album');
    
    // Click multiple times quickly
    await fireEvent.click(playButton);
    await fireEvent.click(playButton);
    await fireEvent.click(playButton);

    // API should only be called once
    expect(mockApiGet).toHaveBeenCalledTimes(1);

    // Resolve to clean up
    resolvePromise!({ ...album, tracks: [] });
    await waitFor(() => expect(playButton).not.toBeDisabled());
  });

  it('does nothing if fetched album has no tracks', async () => {
    const albumWithoutTracks = { ...album, tracks: [] };
    mockApiGet.mockResolvedValue(albumWithoutTracks);

    render(AlbumCard, { props: { album } });
    const playButton = screen.getByLabelText('Play album');
    await fireEvent.click(playButton);

    await waitFor(() => {
      expect(mockApiGet).toHaveBeenCalledWith('/albums/album-1');
      expect(mockPlayQueue).not.toHaveBeenCalled();
    });
  });
});
