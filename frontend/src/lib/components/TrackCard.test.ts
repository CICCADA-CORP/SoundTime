import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const mockPlay = vi.fn();
const mockPlayQueue = vi.fn();

vi.mock('$lib/stores/player.svelte', () => ({
  getPlayerStore: () => ({
    play: mockPlay,
    currentTrack: null,
    isPlaying: false,
    volume: 0.8,
    progress: 0,
    duration: 0,
    shuffle: false,
    repeat: 'none' as const,
    pause: vi.fn(),
    resume: vi.fn(),
    togglePlay: vi.fn(),
    seek: vi.fn(),
    setVolume: vi.fn(),
    toggleShuffle: vi.fn(),
    cycleRepeat: vi.fn(),
  }),
}));

vi.mock('$lib/stores/queue.svelte', () => ({
  getQueueStore: () => ({
    playQueue: mockPlayQueue,
    queue: [],
    currentIndex: -1,
    addToQueue: vi.fn(),
    addNext: vi.fn(),
    removeFromQueue: vi.fn(),
    clearQueue: vi.fn(),
  }),
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => key,
}));

import TrackCard from './TrackCard.svelte';

const createTrack = (overrides: Record<string, any> = {}) => ({
  id: 'track-1',
  title: 'Test Track',
  artist_id: 'a1',
  album_id: null,
  track_number: null,
  disc_number: null,
  duration_secs: 180,
  genre: null,
  year: null,
  file_path: '/test.mp3',
  file_size: 1000,
  format: 'mp3',
  bitrate: 320,
  sample_rate: 44100,
  musicbrainz_id: null,
  waveform_data: null,
  uploaded_by: null,
  play_count: 0,
  created_at: '2025-01-01',
  ...overrides,
});

describe('TrackCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ─── Rendering ─────────────────────────────────────────────────────

  it('renders track title', () => {
    const track = createTrack();
    render(TrackCard, { props: { track } });
    expect(screen.getByText('Test Track')).toBeInTheDocument();
  });

  it('renders artist name when provided', () => {
    const track = createTrack({ artist_name: 'Cool Artist' });
    render(TrackCard, { props: { track } });
    expect(screen.getByText('Cool Artist')).toBeInTheDocument();
  });

  it('renders i18n key "track.unknownArtist" when artist_name is undefined', () => {
    const track = createTrack({ artist_name: undefined });
    render(TrackCard, { props: { track } });
    expect(screen.getByText('track.unknownArtist')).toBeInTheDocument();
  });

  it('renders cover image when cover_url is set', () => {
    const track = createTrack({ cover_url: '/covers/test.jpg' });
    render(TrackCard, { props: { track } });
    const img = screen.getByAltText('Test Track');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', '/covers/test.jpg');
  });

  it('renders fallback emoji 🎵 when no cover_url', () => {
    const track = createTrack();
    render(TrackCard, { props: { track } });
    expect(screen.getByText('🎵')).toBeInTheDocument();
  });

  it('does not render img element when no cover_url', () => {
    const track = createTrack();
    const { container } = render(TrackCard, { props: { track } });
    expect(container.querySelector('img')).toBeNull();
  });

  it('renders as a button', () => {
    const track = createTrack();
    render(TrackCard, { props: { track } });
    expect(screen.getByRole('button')).toBeInTheDocument();
  });

  // ─── Click behavior: player.play vs queue.playQueue ────────────────

  it('calls player.play(track) when clicked WITHOUT tracks/index props', async () => {
    const track = createTrack();
    render(TrackCard, { props: { track } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(mockPlay).toHaveBeenCalledTimes(1);
    expect(mockPlay).toHaveBeenCalledWith(track);
    expect(mockPlayQueue).not.toHaveBeenCalled();
  });

  it('calls queue.playQueue(tracks, index) when clicked WITH tracks and index props', async () => {
    const track1 = createTrack({ id: 't1', title: 'Track 1' });
    const track2 = createTrack({ id: 't2', title: 'Track 2' });
    const tracks = [track1, track2];
    render(TrackCard, { props: { track: track2, tracks, index: 1 } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(mockPlayQueue).toHaveBeenCalledTimes(1);
    expect(mockPlayQueue).toHaveBeenCalledWith(tracks, 1);
    expect(mockPlay).not.toHaveBeenCalled();
  });

  it('calls player.play when tracks is provided but index is undefined', async () => {
    const track = createTrack();
    const tracks = [track];
    render(TrackCard, { props: { track, tracks } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(mockPlay).toHaveBeenCalledTimes(1);
    expect(mockPlay).toHaveBeenCalledWith(track);
    expect(mockPlayQueue).not.toHaveBeenCalled();
  });

  it('calls queue.playQueue when index is 0 (falsy but defined)', async () => {
    const track = createTrack();
    const tracks = [track];
    render(TrackCard, { props: { track, tracks, index: 0 } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(mockPlayQueue).toHaveBeenCalledTimes(1);
    expect(mockPlayQueue).toHaveBeenCalledWith(tracks, 0);
    expect(mockPlay).not.toHaveBeenCalled();
  });
});
