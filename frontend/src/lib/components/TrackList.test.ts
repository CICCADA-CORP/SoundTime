import { describe, it, expect, vi, beforeEach } from 'vitest';
import { tick } from 'svelte';

let mockPlayerStore: any;
let mockQueueStore: any;
let mockAuthStore: any;
const mockApiGet = vi.fn().mockResolvedValue([]);
const mockApiPost = vi.fn().mockResolvedValue({});
const mockApiDelete = vi.fn().mockResolvedValue({});

function resetMocks() {
  mockPlayerStore = {
    currentTrack: null as any,
    isPlaying: false,
  };
  mockQueueStore = {
    queue: [],
    currentIndex: -1,
    playQueue: vi.fn(),
    addToQueue: vi.fn(),
    addNext: vi.fn(),
  };
  mockAuthStore = {
    isAuthenticated: false,
    user: null,
    isAdmin: false,
  };
}

resetMocks();

vi.mock('$lib/stores/player.svelte', () => ({
  getPlayerStore: () => mockPlayerStore,
}));

vi.mock('$lib/stores/queue.svelte', () => ({
  getQueueStore: () => mockQueueStore,
}));

vi.mock('$lib/stores/auth.svelte', () => ({
  getAuthStore: () => mockAuthStore,
}));

vi.mock('$lib/api', () => ({
  api: {
    get: (...args: any[]) => mockApiGet(...args),
    post: (...args: any[]) => mockApiPost(...args),
    delete: (...args: any[]) => mockApiDelete(...args),
  },
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => {
    const translations: Record<string, string> = {
      'track.title': 'Title',
      'track.album': 'Album',
      'track.artist': 'Artist',
      'track.plays': 'Plays',
      'track.quality': 'Quality',
      'track.duration': 'Duration',
      'track.options': 'Options',
      'track.playNext': 'Play Next',
      'track.addToQueue': 'Add to Queue',
      'track.addToPlaylist': 'Add to Playlist',
      'track.viewCredits': 'View Credits',
      'track.share': 'Share',
      'track.report': 'Report',
      'track.reportTitle': 'Report Track',
      'track.reportPlaceholder': 'Enter reason',
      'track.reportSent': 'Report sent',
      'track.reportError': 'Error sending report',
      'track.noPlaylists': 'No playlists',
      'track.noCredits': 'No credits',
      'track.genre': 'Genre',
      'track.year': 'Year',
      'track.format': 'Format',
      'track.bitrate': 'Bitrate',
      'track.sampleRate': 'Sample Rate',
      'track.uploadedBy': 'Uploaded by',
      'common.cancel': 'Cancel',
      'common.close': 'Close',
      'common.copy': 'Copy',
      'common.copied': 'Copied!',
      'track.sendReport': 'Send Report',
    };
    return translations[key] ?? key;
  },
}));

vi.mock('./FavoriteButton.svelte', () => ({
  default: function($$anchor: any, $$props?: any) {},
}));

import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import TrackList from './TrackList.svelte';

const createTrack = (overrides: Record<string, any> = {}) => ({
  id: 't1',
  title: 'Song One',
  artist_id: 'a1',
  album_id: 'al1',
  track_number: 1,
  disc_number: null,
  duration_secs: 180,
  genre: 'Rock',
  year: 2024,
  file_path: '/s1.mp3',
  file_size: 1000,
  format: 'mp3',
  bitrate: 320,
  sample_rate: 44100,
  musicbrainz_id: null,
  waveform_data: null,
  uploaded_by: null,
  play_count: 5,
  created_at: '2025-01-01',
  artist_name: 'Artist One',
  album_title: 'Album X',
  ...overrides,
});

describe('TrackList', () => {
  beforeEach(() => {
    resetMocks();
    vi.clearAllMocks();
    mockApiGet.mockResolvedValue([]);
    mockApiPost.mockResolvedValue({});
  });

  // --- Rendering ---
  it('renders column headers', () => {
    render(TrackList, { props: { tracks: [] } });
    expect(screen.getByText('Title')).toBeInTheDocument();
    expect(screen.getByText('Album')).toBeInTheDocument();
    expect(screen.getByText('Duration')).toBeInTheDocument();
    expect(screen.getByText('Plays')).toBeInTheDocument();
    expect(screen.getByText('Quality')).toBeInTheDocument();
  });

  it('renders empty list with no tracks', () => {
    const { container } = render(TrackList, { props: { tracks: [] } });
    const rows = container.querySelectorAll('[role="button"]');
    expect(rows.length).toBe(0);
  });

  it('renders track rows', () => {
    const tracks = [
      createTrack({ id: 't1', title: 'Song One' }),
      createTrack({ id: 't2', title: 'Song Two', duration_secs: 240, format: 'flac', bitrate: 900 }),
    ];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('Song One')).toBeInTheDocument();
    expect(screen.getByText('Song Two')).toBeInTheDocument();
  });

  it('renders album column when showAlbum is true', () => {
    const tracks = [createTrack({ album_title: 'My Album' })];
    render(TrackList, { props: { tracks, showAlbum: true } });
    expect(screen.getByText('My Album')).toBeInTheDocument();
  });

  it('renders artist column when showArtist is true and showAlbum is false', () => {
    const tracks = [createTrack({ artist_name: 'My Artist' })];
    render(TrackList, { props: { tracks, showAlbum: false, showArtist: true } });
    const artistHeaders = screen.getAllByText('Artist');
    expect(artistHeaders.length).toBeGreaterThan(0);
  });

  it('displays formatted duration', () => {
    const tracks = [createTrack({ duration_secs: 185 })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('3:05')).toBeInTheDocument();
  });

  it('displays play count', () => {
    const tracks = [createTrack({ play_count: 42 })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('42')).toBeInTheDocument();
  });

  it('displays bitrate and format', () => {
    const tracks = [createTrack({ bitrate: 320, format: 'mp3' })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('320k')).toBeInTheDocument();
    expect(screen.getByText('mp3')).toBeInTheDocument();
  });

  it('displays track number', () => {
    const tracks = [createTrack({ track_number: 3 })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('shows dash when no bitrate/format', () => {
    const tracks = [createTrack({ bitrate: null, best_bitrate: null, format: null })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('—')).toBeInTheDocument();
  });

  it('shows 0 play count when null', () => {
    const tracks = [createTrack({ play_count: null })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('0')).toBeInTheDocument();
  });

  it('shows best_bitrate when available', () => {
    const tracks = [createTrack({ best_bitrate: 500, bitrate: 320, format: 'flac' })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('500k')).toBeInTheDocument();
  });

  it('shows federated badge for non-local source', () => {
    const tracks = [createTrack({ best_source: 'remote-server', best_bitrate: 320, format: 'mp3' })];
    render(TrackList, { props: { tracks } });
    expect(screen.getByText('Fed')).toBeInTheDocument();
  });

  it('does not show federated badge for local source', () => {
    const tracks = [createTrack({ best_source: 'local', best_bitrate: 320, format: 'mp3' })];
    render(TrackList, { props: { tracks } });
    expect(screen.queryByText('Fed')).not.toBeInTheDocument();
  });

  // --- Interactions ---
  it('plays track on click', async () => {
    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.click(row);
    expect(mockQueueStore.playQueue).toHaveBeenCalledWith(tracks, 0);
  });

  it('plays track on Enter key', async () => {
    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.keyDown(row, { key: 'Enter' });
    expect(mockQueueStore.playQueue).toHaveBeenCalledWith(tracks, 0);
  });

  it('highlights currently playing track', () => {
    const track = createTrack({ id: 'current-track' });
    mockPlayerStore.currentTrack = track;
    const { container } = render(TrackList, { props: { tracks: [track] } });
    const row = container.querySelector('[role="button"]');
    expect(row?.classList.toString()).toContain('bg-');
  });

  // --- Context menu ---
  it('shows right-click context menu', async () => {
    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    expect(screen.getByText('Play Next')).toBeInTheDocument();
    expect(screen.getByText('Add to Queue')).toBeInTheDocument();
    expect(screen.getByText('View Credits')).toBeInTheDocument();
    expect(screen.getByText('Share')).toBeInTheDocument();
  });

  it('play next adds track to queue and closes menu', async () => {
    const track = createTrack();
    const { container } = render(TrackList, { props: { tracks: [track] } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    const playNextBtn = screen.getByText('Play Next');
    await fireEvent.click(playNextBtn);
    expect(mockQueueStore.addNext).toHaveBeenCalledWith(track);
    // Menu should close
    expect(screen.queryByText('Play Next')).not.toBeInTheDocument();
  });

  it('add to queue adds track and closes menu', async () => {
    const track = createTrack();
    const { container } = render(TrackList, { props: { tracks: [track] } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    const addBtn = screen.getByText('Add to Queue');
    await fireEvent.click(addBtn);
    expect(mockQueueStore.addToQueue).toHaveBeenCalledWith(track);
    expect(screen.queryByText('Add to Queue')).not.toBeInTheDocument();
  });

  it('closes context menu on window click', async () => {
    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    expect(screen.getByText('Play Next')).toBeInTheDocument();
    // Click on window to close
    await fireEvent.click(window);
    await tick();
    expect(screen.queryByText('Play Next')).not.toBeInTheDocument();
  });

  // --- Authenticated context menu items ---
  it('shows report option for auth users in context menu', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    expect(screen.getByText('Report')).toBeInTheDocument();
    expect(screen.getByText('Add to Playlist')).toBeInTheDocument();
  });

  // --- Playlist picker ---
  it('opens playlist picker modal', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/playlists')) {
        return Promise.resolve({ data: [
          { id: 'pl1', name: 'My Playlist', user_id: '1' },
        ]});
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    const playlistBtn = screen.getByText('Add to Playlist');
    await fireEvent.click(playlistBtn);

    await waitFor(() => {
      expect(screen.getByText('My Playlist')).toBeInTheDocument();
    });
  });

  it('adds track to playlist', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/playlists')) {
        return Promise.resolve({ data: [
          { id: 'pl1', name: 'My Playlist', user_id: '1' },
        ]});
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack({ id: 'track-42' })];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Add to Playlist'));

    await waitFor(() => {
      expect(screen.getByText('My Playlist')).toBeInTheDocument();
    });

    await fireEvent.click(screen.getByText('My Playlist'));
    await tick();

    expect(mockApiPost).toHaveBeenCalledWith('/playlists/pl1/tracks', { track_id: 'track-42' });
  });

  it('shows empty playlist message', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/playlists')) {
        return Promise.resolve({ data: [] });
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Add to Playlist'));

    await waitFor(() => {
      expect(screen.getByText('No playlists')).toBeInTheDocument();
    });
  });

  // --- Credits modal ---
  it('opens credits modal', async () => {
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/credits')) {
        return Promise.resolve({ artist: 'Test Artist', album: 'Test Album', genre: 'Rock', year: 2024, format: 'mp3', bitrate: 320, sample_rate: 44100 });
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('View Credits'));

    await waitFor(() => {
      expect(screen.getByText('Test Artist')).toBeInTheDocument();
      expect(screen.getByText('Test Album')).toBeInTheDocument();
    });
  });

  it('shows no credits message when credits are null', async () => {
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/credits')) {
        return Promise.reject(new Error('Not found'));
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('View Credits'));

    await waitFor(() => {
      expect(screen.getByText('No credits')).toBeInTheDocument();
    });
  });

  // --- Share modal ---
  it('opens share modal', async () => {
    const tracks = [createTrack({ id: 'share-track' })];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Share'));

    await waitFor(() => {
      expect(screen.getByText('Copy')).toBeInTheDocument();
    });
  });

  it('copies share link', async () => {
    const mockWriteText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText: mockWriteText },
      writable: true,
      configurable: true,
    });

    const tracks = [createTrack({ id: 'share-track' })];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Share'));

    await waitFor(() => {
      expect(screen.getByText('Copy')).toBeInTheDocument();
    });

    await fireEvent.click(screen.getByText('Copy'));
    expect(mockWriteText).toHaveBeenCalledWith(expect.stringContaining('/tracks/share-track'));
  });

  // --- Report modal ---
  it('opens report modal', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Report'));

    await waitFor(() => {
      expect(screen.getByText('Report Track')).toBeInTheDocument();
      expect(screen.getByText('Send Report')).toBeInTheDocument();
    });
  });

  it('submits a report', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };

    const tracks = [createTrack({ id: 'report-track' })];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Report'));

    await waitFor(() => {
      expect(screen.getByText('Report Track')).toBeInTheDocument();
    });

    const textarea = container.querySelector('textarea')!;
    await fireEvent.input(textarea, { target: { value: 'Copyright violation' } });
    await tick();

    const sendBtn = screen.getByText('Send Report');
    await fireEvent.click(sendBtn);

    await waitFor(() => {
      expect(mockApiPost).toHaveBeenCalledWith('/tracks/report-track/report', { reason: 'Copyright violation' });
    });
  });

  it('shows report error on failure', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiPost.mockRejectedValueOnce(new Error('Server error'));

    const tracks = [createTrack({ id: 'report-track' })];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Report'));

    await waitFor(() => {
      expect(screen.getByText('Report Track')).toBeInTheDocument();
    });

    const textarea = container.querySelector('textarea')!;
    await fireEvent.input(textarea, { target: { value: 'Spam content' } });
    await tick();

    await fireEvent.click(screen.getByText('Send Report'));

    await waitFor(() => {
      expect(container.textContent).toContain('Server error');
    });
  });

  // --- Auth state effects ---
  it('checks liked state for authenticated user', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockResolvedValue(['t1']);

    const tracks = [createTrack()];
    render(TrackList, { props: { tracks } });

    await waitFor(() => {
      expect(mockApiGet).toHaveBeenCalledWith(expect.stringContaining('/favorites/check'));
    });
  });

  it('handles liked check failure silently', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockRejectedValue(new Error('fail'));

    const tracks = [createTrack()];
    // Should not throw
    render(TrackList, { props: { tracks } });
    await tick();
  });

  // --- Playing track visual cues ---
  it('shows pause icon for playing track', () => {
    const track = createTrack({ id: 'playing-track' });
    mockPlayerStore.currentTrack = track;
    mockPlayerStore.isPlaying = true;
    const { container } = render(TrackList, { props: { tracks: [track] } });
    // Should have pause SVG (two rectangles)
    const svgs = container.querySelectorAll('svg');
    expect(svgs.length).toBeGreaterThan(0);
  });

  // --- Cancel buttons in modals ---
  it('closes playlist picker with cancel button', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/playlists')) {
        return Promise.resolve({ data: [] });
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Add to Playlist'));

    await waitFor(() => {
      expect(screen.getByText('Cancel')).toBeInTheDocument();
    });

    await fireEvent.click(screen.getByText('Cancel'));
    await tick();
    expect(screen.queryByText('No playlists')).not.toBeInTheDocument();
  });

  it('closes share modal with close button', async () => {
    const tracks = [createTrack({ id: 'share-close' })];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Share'));

    await waitFor(() => {
      expect(screen.getByText('Close')).toBeInTheDocument();
    });

    await fireEvent.click(screen.getByText('Close'));
    await tick();
    expect(screen.queryByText('Copy')).not.toBeInTheDocument();
  });

  // --- Additional branch coverage tests ---

  it('renders empty column when showAlbum=false and showArtist=false', () => {
    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks, showAlbum: false, showArtist: false } });
    expect(container.textContent).toContain('Song One');
  });

  it('falls back to index+1 when track_number is null', () => {
    const track = createTrack({ track_number: null });
    const { container } = render(TrackList, { props: { tracks: [track] } });
    // Should show "1" (index 0 + 1)
    expect(container.textContent).toContain('1');
  });

  it('shows empty string for null artist_name when showArtist', () => {
    const track = createTrack({ artist_name: null });
    const { container } = render(TrackList, { props: { tracks: [track], showArtist: true } });
    expect(container.textContent).toContain('Song One');
  });

  it('shows empty album title when null and showAlbum', () => {
    const track = createTrack({ album_title: null });
    const { container } = render(TrackList, { props: { tracks: [track], showAlbum: true } });
    expect(container.textContent).toContain('Song One');
  });

  it('handles credits with musicbrainz_id', async () => {
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/credits')) {
        return Promise.resolve({
          artist: 'Artist',
          musicbrainz_id: 'abc-123',
          uploaded_by_username: 'uploader1',
        });
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('View Credits'));

    await waitFor(() => {
      expect(screen.getByText('abc-123')).toBeInTheDocument();
      expect(screen.getByText('uploader1')).toBeInTheDocument();
    });
  });

  it('handles credits with year and sample_rate', async () => {
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/credits')) {
        return Promise.resolve({
          genre: 'Electronic',
          year: 2023,
          format: 'flac',
          bitrate: 1411,
          sample_rate: 96000,
        });
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('View Credits'));

    await waitFor(() => {
      expect(screen.getByText('Electronic')).toBeInTheDocument();
      expect(screen.getByText('2023')).toBeInTheDocument();
    });
  });

  it('shows artist name in second column when showAlbum=false but showArtist=true', () => {
    const track = createTrack({ artist_name: 'Column Artist' });
    render(TrackList, { props: { tracks: [track], showAlbum: false, showArtist: true } });
    // "track.artist" header should be present
    expect(screen.getByText('Artist')).toBeInTheDocument();
  });

  it('matches playlist by owner_id', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/playlists')) {
        return Promise.resolve({
          data: [
            { id: 'pl1', name: 'Owned Playlist', user_id: 'other', owner_id: '1' },
          ],
        });
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Add to Playlist'));

    await waitFor(() => {
      expect(screen.getByText('Owned Playlist')).toBeInTheDocument();
    });
  });

  it('handles playlist fetch error', async () => {
    mockAuthStore.isAuthenticated = true;
    mockAuthStore.user = { id: '1', username: 'alice' };
    mockApiGet.mockImplementation((url: string) => {
      if (url.includes('/playlists')) {
        return Promise.reject(new Error('Network error'));
      }
      return Promise.resolve([]);
    });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    await fireEvent.contextMenu(row);
    await fireEvent.click(screen.getByText('Add to Playlist'));

    await waitFor(() => {
      expect(screen.getByText('No playlists')).toBeInTheDocument();
    });
  });

  it('shows best_source badge only when not local', () => {
    const track = createTrack({ best_bitrate: 256, best_source: 'peer1.example.com', format: 'opus' });
    const { container } = render(TrackList, { props: { tracks: [track] } });
    expect(screen.getByText('Fed')).toBeInTheDocument();
  });

  it('does not show badge when best_source is local', () => {
    const track = createTrack({ best_bitrate: 320, best_source: 'local', format: 'mp3' });
    render(TrackList, { props: { tracks: [track] } });
    expect(screen.queryByText('Fed')).not.toBeInTheDocument();
  });

  it('adjusts context menu position when near window edges', async () => {
    // Mock a narrow window
    Object.defineProperty(window, 'innerWidth', { value: 300, writable: true, configurable: true });
    Object.defineProperty(window, 'innerHeight', { value: 300, writable: true, configurable: true });

    const tracks = [createTrack()];
    const { container } = render(TrackList, { props: { tracks } });
    const row = container.querySelector('[role="button"]')!;
    // Right-click near edge
    await fireEvent.contextMenu(row, { clientX: 290, clientY: 290 });
    expect(screen.getByText('Play Next')).toBeInTheDocument();

    // Restore
    Object.defineProperty(window, 'innerWidth', { value: 1024, writable: true, configurable: true });
    Object.defineProperty(window, 'innerHeight', { value: 768, writable: true, configurable: true });
  });
});
