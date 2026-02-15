import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const mockPlay = vi.fn();

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

import QuickPlayCard from './QuickPlayCard.svelte';

const track = {
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
};

describe('QuickPlayCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders track title', () => {
    render(QuickPlayCard, { props: { track } });
    expect(screen.getByText('Test Track')).toBeInTheDocument();
  });

  it('renders as a button', () => {
    render(QuickPlayCard, { props: { track } });
    expect(screen.getByRole('button')).toBeInTheDocument();
  });

  it('renders cover image when cover_url is set', () => {
    const trackWithCover = { ...track, cover_url: '/covers/test.jpg' };
    render(QuickPlayCard, { props: { track: trackWithCover } });
    const img = screen.getByAltText('Test Track');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', '/covers/test.jpg');
  });

  it('renders fallback emoji 🎵 when no cover_url', () => {
    render(QuickPlayCard, { props: { track } });
    expect(screen.getByText('🎵')).toBeInTheDocument();
  });

  it('does not render img element when no cover_url', () => {
    const { container } = render(QuickPlayCard, { props: { track } });
    expect(container.querySelector('img')).toBeNull();
  });

  it('calls player.play(track) when clicked', async () => {
    render(QuickPlayCard, { props: { track } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(mockPlay).toHaveBeenCalledTimes(1);
    expect(mockPlay).toHaveBeenCalledWith(track);
  });
});
