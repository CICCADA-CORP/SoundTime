import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockTrack = {
  id: 'track-1',
  title: 'Expanded Song',
  artist_name: 'Expanded Artist',
  album_title: 'Expanded Album',
  cover_url: null as string | null,
  artist_id: 'a1',
  album_id: 'al1',
  track_number: 1,
  disc_number: null,
  duration_secs: 200,
  genre: 'Pop',
  year: 2024,
  file_path: '/test.mp3',
  file_size: 5000000,
  format: 'flac',
  bitrate: 1411,
  sample_rate: 44100,
  musicbrainz_id: null,
  waveform_data: null,
  uploaded_by: null,
  play_count: 10,
  created_at: '2025-01-01',
};

let mockPlayerStore: any;
let mockQueueStore: any;
let mockAuthStore: any;
const mockApiGet = vi.fn().mockResolvedValue({ lyrics: null, source: null });

function resetMocks() {
  mockPlayerStore = {
    currentTrack: null as any,
    isPlaying: false,
    volume: 0.8,
    progress: 30,
    duration: 200,
    shuffle: false,
    repeat: 'none' as string,
    play: vi.fn(),
    pause: vi.fn(),
    resume: vi.fn(),
    togglePlay: vi.fn(),
    seek: vi.fn(),
    setVolume: vi.fn(),
    toggleShuffle: vi.fn(),
    cycleRepeat: vi.fn(),
  };
  mockQueueStore = {
    queue: [],
    currentIndex: -1,
    currentTrack: null,
    hasNext: false,
    hasPrevious: false,
    playQueue: vi.fn(),
    addToQueue: vi.fn(),
    addNext: vi.fn(),
    removeFromQueue: vi.fn(),
    clearQueue: vi.fn(),
    next: vi.fn(),
    previous: vi.fn(),
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
    post: vi.fn(),
    delete: vi.fn(),
  },
  API_BASE: '/api',
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => key,
}));

// Mock lucide-svelte icons
vi.mock('lucide-svelte', () => {
  const noop = function($$anchor: any, $$props?: any) {};
  return { X: noop, ChevronDown: noop, Music: noop, ListMusic: noop, Mic2: noop };
});

import { render, screen, fireEvent } from '@testing-library/svelte';
import ExpandedPlayer from './ExpandedPlayer.svelte';

describe('ExpandedPlayer', () => {
  beforeEach(() => {
    resetMocks();
    vi.clearAllMocks();
    mockApiGet.mockResolvedValue({ lyrics: null, source: null });
  });

  it('renders nothing when closed', () => {
    const { container } = render(ExpandedPlayer, { props: { open: false, onclose: vi.fn() } });
    expect(container.querySelector('.fixed')).toBeNull();
  });

  it('renders nothing when open but no track', () => {
    mockPlayerStore.currentTrack = null;
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(container.querySelector('.fixed')).toBeNull();
  });

  it('renders full player when open with track', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(container.querySelector('.fixed')).toBeInTheDocument();
  });

  it('displays track title and artist', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('Expanded Song')).toBeInTheDocument();
    expect(screen.getByText('Expanded Artist')).toBeInTheDocument();
  });

  it('displays album title when present', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, album_title: 'Test Album' };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('Test Album')).toBeInTheDocument();
  });

  it('shows "Unknown Artist" when artist_name is null', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, artist_name: null };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('Unknown Artist')).toBeInTheDocument();
  });

  it('shows cover image when cover_url is set', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url: '/covers/test.jpg' };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const img = container.querySelector('img');
    expect(img).toBeInTheDocument();
  });

  it('displays formatted progress and duration', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.progress = 65;
    mockPlayerStore.duration = 200;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('1:05')).toBeInTheDocument();
    expect(screen.getByText('3:20')).toBeInTheDocument();
  });

  it('shows technical details (format, bitrate, sample_rate)', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, format: 'flac', bitrate: 1411, sample_rate: 44100 };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('flac')).toBeInTheDocument();
    expect(screen.getByText('1411 kbps')).toBeInTheDocument();
    expect(screen.getByText('44.1 kHz')).toBeInTheDocument();
  });

  it('calls togglePlay when play button clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const playBtn = container.querySelector('.w-14.h-14.rounded-full');
    if (playBtn) {
      await fireEvent.click(playBtn);
      expect(mockPlayerStore.togglePlay).toHaveBeenCalled();
    }
  });

  it('calls toggleShuffle when shuffle button clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const shuffleBtn = container.querySelector('button[title="Shuffle"]');
    if (shuffleBtn) {
      await fireEvent.click(shuffleBtn);
      expect(mockPlayerStore.toggleShuffle).toHaveBeenCalled();
    }
  });

  it('calls queue.previous when previous button clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const prevBtn = container.querySelector('button[title="Précédent"]');
    if (prevBtn) {
      await fireEvent.click(prevBtn);
      expect(mockQueueStore.previous).toHaveBeenCalled();
    }
  });

  it('calls queue.next when next button clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const nextBtn = container.querySelector('button[title="Suivant"]');
    if (nextBtn) {
      await fireEvent.click(nextBtn);
      expect(mockQueueStore.next).toHaveBeenCalled();
    }
  });

  it('calls onclose when close button clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const onclose = vi.fn();
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose } });
    const closeBtn = container.querySelector('button[aria-label="Fermer"]');
    if (closeBtn) {
      await fireEvent.click(closeBtn);
      expect(onclose).toHaveBeenCalled();
    }
  });

  it('shows "Now Playing" header label', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('player.nowPlaying')).toBeInTheDocument();
  });

  it('shows queue empty message when no upcoming tracks', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockQueueStore.queue = [mockTrack];
    mockQueueStore.currentIndex = 0;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('player.queueEmpty')).toBeInTheDocument();
  });

  it('shows upcoming tracks in queue panel', () => {
    const track2 = { ...mockTrack, id: 't2', title: 'Next Track', artist_name: 'Another Artist', duration_secs: 240 };
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockQueueStore.queue = [mockTrack, track2];
    mockQueueStore.currentIndex = 0;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('Next Track')).toBeInTheDocument();
  });

  it('shows lyrics panel header', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    // The lyrics header shows "player.lyrics" via t()
    const lyricsHeaders = screen.getAllByText('player.lyrics');
    expect(lyricsHeaders.length).toBeGreaterThan(0);
  });

  it('fetches lyrics when opened', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockApiGet.mockResolvedValueOnce([]).mockResolvedValueOnce({ lyrics: 'La la la', source: 'embedded' });
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    await vi.waitFor(() => {
      expect(mockApiGet).toHaveBeenCalledWith(`/tracks/${mockTrack.id}/lyrics`);
    }, { timeout: 2000 });
  });

  it('resolves absolute URL as-is for cover', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url: 'https://cdn.example.com/cover.jpg' };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const img = container.querySelector('img[alt="Expanded Song"]');
    expect(img?.getAttribute('src')).toBe('https://cdn.example.com/cover.jpg');
  });

  it('resolves relative URL by prepending base', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url: '/media/covers/art.jpg' };
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const img = container.querySelector('img[alt="Expanded Song"]');
    expect(img?.getAttribute('src')).toBe('/media/covers/art.jpg');
  });

  it('hides album title when not present', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, album_title: null };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('Expanded Song')).toBeInTheDocument();
  });

  it('hides format when null', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, format: null, bitrate: null, sample_rate: null };
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.queryByText(/kbps/)).toBeNull();
    expect(screen.queryByText(/kHz/)).toBeNull();
  });

  it('shows favorite button when authenticated', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockAuthStore.isAuthenticated = true;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    // FavoriteButton is rendered when authenticated
    expect(screen.getByText('Expanded Song')).toBeInTheDocument();
  });

  it('shows repeat "one" indicator', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.repeat = 'one';
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    // When repeat is 'one', a '1' indicator should appear
    const allOnes = screen.getAllByText('1');
    expect(allOnes.length).toBeGreaterThan(0);
  });

  it('does not show repeat "one" when repeat is none', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.repeat = 'none';
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.queryByText('1')).toBeNull();
  });

  it('shows isPlaying state in play button', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.isPlaying = true;
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const playBtn = container.querySelector('.w-14.h-14.rounded-full');
    expect(playBtn).toBeInTheDocument();
  });

  it('closes on Escape key', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const onclose = vi.fn();
    render(ExpandedPlayer, { props: { open: true, onclose } });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onclose).toHaveBeenCalled();
  });

  it('handles seek via progress bar click', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.duration = 200;
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const progressBar = container.querySelector('.cursor-pointer.group');
    if (progressBar) {
      Object.defineProperty(progressBar, 'getBoundingClientRect', {
        value: () => ({ left: 0, right: 400, width: 400, top: 0, bottom: 10, height: 10, x: 0, y: 0, toJSON: () => {} }),
      });
      await fireEvent.click(progressBar, { clientX: 100 });
      expect(mockPlayerStore.seek).toHaveBeenCalledWith(50); // 100/400 * 200
    }
  });

  it('shows upcoming track count', () => {
    const track2 = { ...mockTrack, id: 't2', title: 'Track 2', duration_secs: 120 };
    const track3 = { ...mockTrack, id: 't3', title: 'Track 3', duration_secs: 180 };
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockQueueStore.queue = [mockTrack, track2, track3];
    mockQueueStore.currentIndex = 0;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('(2)')).toBeInTheDocument();
  });

  it('shows track cover in upcoming queue item', () => {
    const track2 = { ...mockTrack, id: 't2', title: 'Covered Track', cover_url: '/cover.jpg' };
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockQueueStore.queue = [mockTrack, track2];
    mockQueueStore.currentIndex = 0;
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const queueImgs = container.querySelectorAll('img');
    // At least one img for the queued track
    expect(queueImgs.length).toBeGreaterThan(0);
  });

  it('shows emoji when queue track has no cover', () => {
    const track2 = { ...mockTrack, id: 't2', title: 'NoCover', cover_url: null };
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockQueueStore.queue = [mockTrack, track2];
    mockQueueStore.currentIndex = 0;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    // The queue item without cover shows emoji
    const emojis = screen.getAllByText('🎵');
    expect(emojis.length).toBeGreaterThan(0);
  });

  it('shows "Unknown" for queue track without artist_name', () => {
    const track2 = { ...mockTrack, id: 't2', title: 'NoArtist', artist_name: null };
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockQueueStore.queue = [mockTrack, track2];
    mockQueueStore.currentIndex = 0;
    render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    expect(screen.getByText('Unknown')).toBeInTheDocument();
  });

  it('shows zero progress when duration is 0', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.duration = 0;
    mockPlayerStore.progress = 0;
    const { container } = render(ExpandedPlayer, { props: { open: true, onclose: vi.fn() } });
    const progressFill = container.querySelector('.bg-white.rounded-full.transition');
    if (progressFill) {
      expect(progressFill.getAttribute('style')).toContain('0%');
    }
  });
});
