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
let mockRadioStore: any;

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
    autoplay: false,
    playQueue: vi.fn(),
    addToQueue: vi.fn(),
    addNext: vi.fn(),
    removeFromQueue: vi.fn(),
    clearQueue: vi.fn(),
    next: vi.fn(),
    previous: vi.fn(),
    toggleAutoplay: vi.fn(),
  };
  mockAuthStore = {
    isAuthenticated: false,
    user: null,
    isAdmin: false,
  };
  mockRadioStore = {
    active: false,
    seedType: null,
    seedLabel: '',
    loading: false,
    exhausted: false,
    error: null,
    playedCount: 0,
    autoplayMode: false,
    startRadio: vi.fn(),
    stopRadio: vi.fn(),
    fetchMoreTracks: vi.fn(),
    markPlayed: vi.fn(),
    clearError: vi.fn(),
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

vi.mock('$lib/stores/radio.svelte', () => ({
  getRadioStore: () => mockRadioStore,
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

  function withTrack() {
    mockPlayerStore.currentTrack = { ...mockTrack };
    return render(AudioPlayer);
  }

  it('hides the mini player until a track is selected', () => {
    render(AudioPlayer);
    expect(screen.queryByTestId('audio-player')).toBeNull();
  });

  it('shows the current title, artist and times', () => {
    mockPlayerStore.progress = 65;
    mockPlayerStore.duration = 180;
    withTrack();
    for (const text of ['Test Song', 'Test Artist', '1:05', '3:00']) {
      expect(screen.getByText(text)).toBeInTheDocument();
    }
  });

  it('keeps the track and its expand button available without an artist or artwork', () => {
    mockPlayerStore.currentTrack = { ...mockTrack, artist_name: null };
    render(AudioPlayer);
    expect(screen.queryByText('track.unknownArtist')).toBeNull();
    expect(screen.getByText(mockTrack.title)).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'player.expand' }).length).toBeGreaterThan(0);
  });

  it.each(['/covers/test.jpg', 'https://cdn.example.com/cover.jpg'])('displays the artwork at %s', (cover_url) => {
    mockPlayerStore.currentTrack = { ...mockTrack, cover_url };
    const { container } = render(AudioPlayer);
    expect(container.querySelector('img')).toHaveAttribute('src', cover_url);
  });

  it.each([
    ['shell.play', 'player', 'togglePlay'], ['player.previous', 'queue', 'previous'],
    ['player.next', 'queue', 'next'], ['player.shuffle', 'player', 'toggleShuffle'],
    ['player.autoplay', 'queue', 'toggleAutoplay'], ['player.repeat · none', 'player', 'cycleRepeat'],
  ])('wires the %s control to playback', async (label, store, action) => {
    withTrack();
    await fireEvent.click(screen.getByRole('button', { name: label }));
    expect((store === 'player' ? mockPlayerStore : mockQueueStore)[action]).toHaveBeenCalledOnce();
  });

  it('announces a stream error and offers a named retry on the selected track', async () => {
    mockPlayerStore.failed = true;
    withTrack();
    expect(screen.getByRole('alert')).toHaveTextContent('player.playbackError');
    await fireEvent.click(screen.getByRole('button', { name: 'radio.tryAgain' }));
    expect(mockPlayerStore.togglePlay).toHaveBeenCalledOnce();
  });

  it('announces pause during playback and pressed shuffle and repeat states', () => {
    Object.assign(mockPlayerStore, { isPlaying: true, shuffle: true, repeat: 'one' });
    withTrack();
    expect(screen.getByRole('button', { name: 'shell.pause' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'player.shuffle' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTitle('player.repeat · one')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('1')).toBeInTheDocument();
  });

  it('passes slider changes to seek and volume', async () => {
    mockPlayerStore.duration = 180;
    withTrack();
    await fireEvent.input(screen.getByRole('slider', { name: 'shell.seek' }), { target: { value: '90' } });
    await fireEvent.input(screen.getByRole('slider', { name: 'player.volume' }), { target: { value: '0.3' } });
    expect(mockPlayerStore.seek).toHaveBeenCalledWith(90);
    expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(0.3);
  });

  it('disables seeking until the duration is known', () => {
    withTrack();
    expect(screen.getByRole('slider', { name: 'shell.seek' })).toBeDisabled();
  });

  it('restores the chosen listening level after muting', async () => {
    mockPlayerStore.volume = 0.35;
    mockPlayerStore.setVolume.mockImplementation((value: number) => { mockPlayerStore.volume = value; });
    withTrack();
    const mute = screen.getByRole('button', { name: 'player.mute' });
    await fireEvent.click(mute);
    expect(mockPlayerStore.volume).toBe(0);
    await fireEvent.click(mute);
    expect(mockPlayerStore.volume).toBe(0.35);
  });

  it.each([[false, 'radio.nowPlaying'], [true, 'player.autoplayActive']] as const)('shows the active radio mode', (autoplayMode, label) => {
    Object.assign(mockRadioStore, { active: true, autoplayMode });
    withTrack();
    expect(screen.getByText(label)).toBeInTheDocument();
  });
});
