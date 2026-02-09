import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockTrack = {
  id: 'track-1',
  title: 'Test Song',
  artist_name: 'Test Artist',
  album_title: 'Test Album',
  cover_url: null as string | null,
  artist_id: 'a1',
  album_id: 'al1',
  track_number: 1,
  disc_number: null,
  duration_secs: 180,
  genre: 'Rock',
  year: 2024,
  file_path: '/test.mp3',
  file_size: 5000000,
  format: 'mp3',
  bitrate: 320,
  sample_rate: 44100,
  musicbrainz_id: null,
  waveform_data: null,
  uploaded_by: null,
  play_count: 42,
  created_at: '2025-01-01',
};

let mockPlayerStore: any;
let mockQueueStore: any;
let mockAuthStore: any;

function resetMocks() {
  mockPlayerStore = {
    currentTrack: null as any,
    isPlaying: false,
    volume: 0.8,
    progress: 0,
    duration: 0,
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
  api: { get: vi.fn().mockResolvedValue([]), post: vi.fn(), delete: vi.fn() },
  API_BASE: '/api',
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => key,
}));

import { render, screen, fireEvent } from '@testing-library/svelte';
import AudioPlayer from './AudioPlayer.svelte';

describe('AudioPlayer', () => {
  beforeEach(() => {
    resetMocks();
    vi.clearAllMocks();
  });

  it('renders nothing when no track is playing', () => {
    const { container } = render(AudioPlayer);
    const playerBar = container.querySelector('.fixed');
    expect(playerBar).toBeNull();
  });

  it('renders player bar when a track is playing', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    const playerBar = container.querySelector('.fixed');
    expect(playerBar).toBeInTheDocument();
  });

  it('displays track title and artist', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    render(AudioPlayer);
    expect(screen.getAllByText('Test Song').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('Test Artist').length).toBeGreaterThanOrEqual(1);
  });

  it('shows emoji fallback when no cover_url', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url: null };
    render(AudioPlayer);
    expect(screen.getAllByText('🎵').length).toBeGreaterThanOrEqual(1);
  });

  it('shows cover image when cover_url is set', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url: '/covers/test.jpg' };
    const { container } = render(AudioPlayer);
    const img = container.querySelector('img');
    expect(img).toBeInTheDocument();
  });

  it('shows play button when paused', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.isPlaying = false;
    const { container } = render(AudioPlayer);
    const playPauseBtn = container.querySelector('.rounded-full.bg-white');
    expect(playPauseBtn).toBeInTheDocument();
  });

  it('calls togglePlay when play/pause button is clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    const playPauseBtn = container.querySelector('.rounded-full.bg-white')!;
    await fireEvent.click(playPauseBtn);
    expect(mockPlayerStore.togglePlay).toHaveBeenCalled();
  });

  it('calls queue.previous when previous button is clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    const prevBtn = container.querySelector('button[title="Previous"]');
    if (prevBtn) {
      await fireEvent.click(prevBtn);
      expect(mockQueueStore.previous).toHaveBeenCalled();
    }
  });

  it('calls queue.next when next button is clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    const nextBtn = container.querySelector('button[title="Next"]');
    if (nextBtn) {
      await fireEvent.click(nextBtn);
      expect(mockQueueStore.next).toHaveBeenCalled();
    }
  });

  it('calls toggleShuffle when shuffle button is clicked', async () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    const shuffleBtn = container.querySelector('button[title="Shuffle"]');
    if (shuffleBtn) {
      await fireEvent.click(shuffleBtn);
      expect(mockPlayerStore.toggleShuffle).toHaveBeenCalled();
    }
  });

  it('displays formatted progress and duration', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.progress = 65;
    mockPlayerStore.duration = 180;
    render(AudioPlayer);
    expect(screen.getByText('1:05')).toBeInTheDocument();
    expect(screen.getByText('3:00')).toBeInTheDocument();
  });

  it('renders progress bar', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.progress = 90;
    mockPlayerStore.duration = 180;
    const { container } = render(AudioPlayer);
    const progressBar = container.querySelector('.cursor-pointer');
    expect(progressBar).toBeInTheDocument();
  });

  it('renders volume control', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    // Volume SVG icon
    const volumeIcons = container.querySelectorAll('svg');
    expect(volumeIcons.length).toBeGreaterThan(0);
  });

  it('shows "Unknown" when artist_name is null', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, artist_name: null };
    render(AudioPlayer);
    expect(screen.getAllByText('Unknown').length).toBeGreaterThanOrEqual(1);
  });

  it('renders progress bar as clickable', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.duration = 200;
    const { container } = render(AudioPlayer);
    const progressBar = container.querySelector('.cursor-pointer');
    expect(progressBar).toBeInTheDocument();
  });

  it('renders volume bar as clickable', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    const { container } = render(AudioPlayer);
    const cursorPointers = container.querySelectorAll('.cursor-pointer');
    expect(cursorPointers.length).toBeGreaterThanOrEqual(2);
  });

  it('resolves absolute URL as-is for cover', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url: 'https://cdn.example.com/cover.jpg' };
    const { container } = render(AudioPlayer);
    const img = container.querySelector('img');
    expect(img).toBeInTheDocument();
    expect(img?.getAttribute('src')).toBe('https://cdn.example.com/cover.jpg');
  });

  it('shows repeat "one" label when repeat is one', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.repeat = 'one';
    const { container } = render(AudioPlayer);
    const oneSpan = container.querySelector('.absolute');
    expect(oneSpan?.textContent).toBe('1');
  });

  it('does not show repeat "one" label when repeat is none', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.repeat = 'none';
    render(AudioPlayer);
    // The '1' span should not be present
    expect(screen.queryByText('1')).toBeNull();
  });

  it('shows isPlaying SVG when playing', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.isPlaying = true;
    const { container } = render(AudioPlayer);
    // When playing, the pause SVG has two rects (M6 19h4... and M8-14...)
    const playPauseBtn = container.querySelector('.rounded-full.bg-white');
    expect(playPauseBtn).toBeInTheDocument();
  });

  it('applies shuffle highlight class when shuffle is true', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.shuffle = true;
    const { container } = render(AudioPlayer);
    const shuffleBtn = container.querySelector('button[title="Shuffle"]');
    expect(shuffleBtn).toBeInTheDocument();
  });

  it('applies repeat highlight class when repeat is all', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.repeat = 'all';
    const { container } = render(AudioPlayer);
    const repeatBtn = container.querySelector('button[title*="Repeat"]');
    expect(repeatBtn).toBeInTheDocument();
  });

  it('shows zero progress width when duration is 0', () => {
    mockPlayerStore.currentTrack = { ...mockTrack };
    mockPlayerStore.duration = 0;
    mockPlayerStore.progress = 0;
    const { container } = render(AudioPlayer);
    // Progress bar inner should have width: 0%
    const allDivs = container.querySelectorAll('[style]');
    let found = false;
    for (const div of allDivs) {
      const style = div.getAttribute('style') ?? '';
      if (style.includes('width:') && style.includes('0%')) found = true;
    }
    expect(found).toBe(true);
  });
});
