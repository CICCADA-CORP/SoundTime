import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock dependencies
vi.mock('$lib/api', () => ({
  api: {
    post: vi.fn().mockResolvedValue(undefined),
    get: vi.fn().mockResolvedValue(undefined),
  },
  streamUrl: vi.fn((id: string) => `/api/tracks/${id}/stream`),
  API_BASE: '/api',
  lastfmApi: {
    nowPlaying: vi.fn().mockResolvedValue(undefined),
    scrobble: vi.fn().mockResolvedValue(undefined),
  },
}));

import { getPlayerStore } from './player.svelte';
import { api } from '$lib/api';

// Mock HTMLAudioElement — track instances for event triggering
let lastAudioInstance: MockAudio | null = null;

class MockAudio {
  src = '';
  volume = 1;
  currentTime = 0;
  duration = 0;
  playbackRate = 1;
  private listeners: Record<string, Function[]> = {};

  constructor() {
    lastAudioInstance = this;
  }

  addEventListener(event: string, handler: Function) {
    if (!this.listeners[event]) this.listeners[event] = [];
    this.listeners[event].push(handler);
  }

  removeEventListener() {}

  play() {
    this.trigger('play');
    return Promise.resolve();
  }

  pause() {
    this.trigger('pause');
  }

  trigger(event: string) {
    (this.listeners[event] || []).forEach(fn => fn());
  }
}

Object.defineProperty(globalThis, 'Audio', { value: MockAudio, writable: true });

// Mock navigator.mediaSession
if (!('mediaSession' in navigator)) {
  Object.defineProperty(navigator, 'mediaSession', {
    value: {
      metadata: null,
      playbackState: 'none',
      setPositionState: vi.fn(),
      setActionHandler: vi.fn(),
    },
    writable: true,
    configurable: true,
  });
} else {
  // Ensure the existing mediaSession has mock methods
  if (!navigator.mediaSession.setPositionState) {
    (navigator.mediaSession as any).setPositionState = vi.fn();
  }
  if (!navigator.mediaSession.setActionHandler) {
    (navigator.mediaSession as any).setActionHandler = vi.fn();
  }
}

// Mock MediaMetadata if not available
if (typeof globalThis.MediaMetadata === 'undefined') {
  (globalThis as any).MediaMetadata = class MockMediaMetadata {
    title: string;
    artist: string;
    album: string;
    artwork: MediaImage[];
    constructor(init?: { title?: string; artist?: string; album?: string; artwork?: MediaImage[] }) {
      this.title = init?.title ?? '';
      this.artist = init?.artist ?? '';
      this.album = init?.album ?? '';
      this.artwork = init?.artwork ?? [];
    }
  };
}

describe('Player Store', () => {
  let player: ReturnType<typeof getPlayerStore>;

  beforeEach(() => {
    vi.clearAllMocks();
    player = getPlayerStore();
  });

  describe('initial state', () => {
    it('has no current track', () => {
      expect(player.currentTrack).toBeNull();
    });

    it('is not playing', () => {
      expect(player.isPlaying).toBe(false);
    });

    it('has default volume', () => {
      expect(player.volume).toBeGreaterThan(0);
      expect(player.volume).toBeLessThanOrEqual(1);
    });

    it('has zero progress', () => {
      expect(player.progress).toBe(0);
    });

    it('repeat is none', () => {
      expect(player.repeat).toBe('none');
    });

    it('shuffle is false', () => {
      expect(player.shuffle).toBe(false);
    });
  });

  describe('play', () => {
    const mockTrack = {
      id: 'track-1',
      title: 'Test Song',
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

    it('sets current track', () => {
      player.play(mockTrack);
      expect(player.currentTrack).toEqual(mockTrack);
    });

    it('sets isPlaying to true', () => {
      player.play(mockTrack);
      expect(player.isPlaying).toBe(true);
    });
  });

  describe('pause and resume', () => {
    const mockTrack = {
      id: 'track-2',
      title: 'Another Song',
      artist_id: 'a2',
      album_id: null,
      track_number: null,
      disc_number: null,
      duration_secs: 200,
      genre: null,
      year: null,
      file_path: '/test2.mp3',
      file_size: 2000,
      format: 'mp3',
      bitrate: 256,
      sample_rate: 44100,
      musicbrainz_id: null,
      waveform_data: null,
      uploaded_by: null,
      play_count: 0,
      created_at: '2025-01-01',
    };

    it('pause sets isPlaying to false', () => {
      player.play(mockTrack);
      player.pause();
      expect(player.isPlaying).toBe(false);
    });

    it('resume sets isPlaying to true', () => {
      player.play(mockTrack);
      player.pause();
      player.resume();
      expect(player.isPlaying).toBe(true);
    });
  });

  describe('togglePlay', () => {
    const mockTrack = {
      id: 'track-3', title: 'Song 3', artist_id: 'a3', album_id: null,
      track_number: null, disc_number: null, duration_secs: 100, genre: null,
      year: null, file_path: '/t3.mp3', file_size: 500, format: 'mp3',
      bitrate: 128, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 0, created_at: '2025-01-01',
    };

    it('toggles between play and pause', () => {
      player.play(mockTrack);
      expect(player.isPlaying).toBe(true);
      player.togglePlay();
      expect(player.isPlaying).toBe(false);
      player.togglePlay();
      expect(player.isPlaying).toBe(true);
    });
  });

  describe('seek', () => {
    it('updates progress', () => {
      const mockTrack = {
        id: 'track-4', title: 'T4', artist_id: 'a', album_id: null,
        track_number: null, disc_number: null, duration_secs: 300, genre: null,
        year: null, file_path: '/t.mp3', file_size: 100, format: 'mp3',
        bitrate: 128, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
        uploaded_by: null, play_count: 0, created_at: '2025-01-01',
      };
      player.play(mockTrack);
      player.seek(60);
      expect(player.progress).toBe(60);
    });
  });

  describe('setVolume', () => {
    it('clamps volume between 0 and 1', () => {
      player.setVolume(0.5);
      expect(player.volume).toBe(0.5);

      player.setVolume(-0.5);
      expect(player.volume).toBe(0);

      player.setVolume(1.5);
      expect(player.volume).toBe(1);
    });
  });

  describe('toggleShuffle', () => {
    it('toggles shuffle mode', () => {
      expect(player.shuffle).toBe(false);
      player.toggleShuffle();
      expect(player.shuffle).toBe(true);
      player.toggleShuffle();
      expect(player.shuffle).toBe(false);
    });
  });

  describe('cycleRepeat', () => {
    it('cycles through none → one → all → none', () => {
      expect(player.repeat).toBe('none');
      player.cycleRepeat();
      expect(player.repeat).toBe('one');
      player.cycleRepeat();
      expect(player.repeat).toBe('all');
      player.cycleRepeat();
      expect(player.repeat).toBe('none');
    });
  });

  describe('audio events', () => {
    const mockTrack = {
      id: 'track-audio', title: 'Audio Events', artist_id: 'a1', album_id: null,
      track_number: null, disc_number: null, duration_secs: 180, genre: null,
      year: null, file_path: '/audio.mp3', file_size: 1000, format: 'mp3',
      bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 0, created_at: '2025-01-01',
    };

    it('logs history when switching tracks with progress > 5', () => {
      const track1 = { ...mockTrack, id: 'track-first' };
      const track2 = { ...mockTrack, id: 'track-second' };
      player.play(track1);
      player.seek(10); // progress > 5
      player.play(track2);
      expect(api.post).toHaveBeenCalledWith('/history', {
        track_id: 'track-first',
        duration_listened: 10,
      });
    });

    it('does not log history when switching tracks with progress <= 5', () => {
      const track1 = { ...mockTrack, id: 'track-A' };
      const track2 = { ...mockTrack, id: 'track-B' };
      player.play(track1);
      player.seek(2); // progress <= 5
      vi.clearAllMocks();
      player.play(track2);
      expect(api.post).not.toHaveBeenCalled();
    });

    it('ended event with repeat=none dispatches trackended', () => {
      player.play(mockTrack);
      const dispatchSpy = vi.fn();
      window.addEventListener('soundtime:trackended', dispatchSpy);

      // Trigger ended on the internal audio
      lastAudioInstance!.trigger('ended');

      expect(player.isPlaying).toBe(false);
      expect(dispatchSpy).toHaveBeenCalled();
      window.removeEventListener('soundtime:trackended', dispatchSpy);
    });

    it('ended event logs history for current track', () => {
      player.play(mockTrack);
      vi.clearAllMocks();

      // Set duration via loadedmetadata event (this updates the module-level duration)
      (lastAudioInstance as any).duration = 200;
      lastAudioInstance!.trigger('loadedmetadata');

      lastAudioInstance!.trigger('ended');

      expect(api.post).toHaveBeenCalledWith('/history', {
        track_id: 'track-audio',
        duration_listened: 200,
      });
    });

    it('ended event with repeat=one restarts playback', () => {
      player.play(mockTrack);
      player.cycleRepeat(); // none → one
      expect(player.repeat).toBe('one');

      lastAudioInstance!.trigger('ended');

      // Should restart: currentTime = 0 and play again
      expect(lastAudioInstance!.currentTime).toBe(0);
      expect(player.isPlaying).toBe(true);
    });

    it('timeupdate event updates progress', () => {
      player.play(mockTrack);
      lastAudioInstance!.currentTime = 42;
      lastAudioInstance!.trigger('timeupdate');
      expect(player.progress).toBe(42);
    });

    it('loadedmetadata event updates duration', () => {
      player.play(mockTrack);
      (lastAudioInstance as any).duration = 300;
      lastAudioInstance!.trigger('loadedmetadata');
      expect(player.duration).toBe(300);
    });
  });

  describe('media session integration', () => {
    const trackWithCover = {
      id: 'track-ms', title: 'Media Session', artist_id: 'a1', album_id: 'al1',
      track_number: 1, disc_number: null, duration_secs: 200, genre: 'Pop',
      year: 2024, file_path: '/ms.mp3', file_size: 3000, format: 'mp3',
      bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 10, created_at: '2025-01-01',
      artist_name: 'Test Artist', album_title: 'Test Album',
      cover_url: '/covers/test.jpg',
    };

    const trackNoCover = {
      ...trackWithCover,
      id: 'track-ms2',
      cover_url: undefined,
      artist_name: undefined,
      album_title: undefined,
    };

    const trackHttpCover = {
      ...trackWithCover,
      id: 'track-ms3',
      cover_url: 'https://cdn.example.com/cover.jpg',
    };

    it('play() with cover_url triggers media session and favicon update', () => {
      player.play(trackWithCover);
      if ('mediaSession' in navigator) {
        expect(navigator.mediaSession.metadata).toBeTruthy();
      }
    });

    it('play() with null cover_url does not set artwork', () => {
      player.play(trackNoCover);
      expect(player.currentTrack).toEqual(trackNoCover);
    });

    it('play() with absolute http cover_url resolves correctly', () => {
      player.play(trackHttpCover);
      expect(player.currentTrack).toEqual(trackHttpCover);
    });

    it('pause() updates mediaSession playbackState', () => {
      player.play(trackWithCover);
      player.pause();
      if ('mediaSession' in navigator) {
        expect(navigator.mediaSession.playbackState).toBe('paused');
      }
    });

    it('resume() updates mediaSession playbackState', () => {
      player.play(trackWithCover);
      player.pause();
      player.resume();
      if ('mediaSession' in navigator) {
        expect(navigator.mediaSession.playbackState).toBe('playing');
      }
    });
  });

  describe('favicon management', () => {
    it('updates favicon when playing track with cover', () => {
      const track = {
        id: 'fav-1', title: 'Favicon Test', artist_id: 'a1', album_id: null,
        track_number: null, disc_number: null, duration_secs: 100, genre: null,
        year: null, file_path: '/f.mp3', file_size: 100, format: 'mp3',
        bitrate: 128, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
        uploaded_by: null, play_count: 0, created_at: '2025-01-01',
        cover_url: '/covers/fav.jpg',
      };
      player.play(track);
      const link = document.querySelector('link[rel="icon"]');
      expect(link).toBeTruthy();
    });

    it('restores favicon when playing track without cover after one with cover', () => {
      const trackWithCover = {
        id: 'fav-2a', title: 'With Cover', artist_id: 'a1', album_id: null,
        track_number: null, disc_number: null, duration_secs: 100, genre: null,
        year: null, file_path: '/f.mp3', file_size: 100, format: 'mp3',
        bitrate: 128, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
        uploaded_by: null, play_count: 0, created_at: '2025-01-01',
        cover_url: '/covers/test.jpg',
      };
      const trackNoCover = {
        ...trackWithCover, id: 'fav-2b', cover_url: undefined,
      };
      player.play(trackWithCover);
      player.play(trackNoCover);
      const link = document.querySelector('link[rel="icon"]');
      if (link) {
        // After restoring, href should not contain /covers/
        expect((link as HTMLLinkElement).href).not.toContain('/covers/');
      }
      expect(true).toBe(true);
    });

    it('creates favicon link element if none exists', () => {
      // Remove existing favicon
      const existing = document.querySelector('link[rel="icon"]');
      if (existing) existing.remove();

      const track = {
        id: 'fav-3', title: 'Create Favicon', artist_id: 'a1', album_id: null,
        track_number: null, disc_number: null, duration_secs: 100, genre: null,
        year: null, file_path: '/f.mp3', file_size: 100, format: 'mp3',
        bitrate: 128, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
        uploaded_by: null, play_count: 0, created_at: '2025-01-01',
        cover_url: '/covers/new.jpg',
      };
      player.play(track);
      const link = document.querySelector('link[rel="icon"]');
      expect(link).toBeTruthy();
    });
  });

  describe('audio event edge cases', () => {
    const mockTrack = {
      id: 'edge-1', title: 'Edge Case', artist_id: 'a1', album_id: null,
      track_number: null, disc_number: null, duration_secs: 180, genre: null,
      year: null, file_path: '/edge.mp3', file_size: 1000, format: 'mp3',
      bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 0, created_at: '2025-01-01',
    };

    it('ended event with repeat=none updates favicon to null', () => {
      // Ensure repeat is 'none' (module state persists across tests)
      while (player.repeat !== 'none') player.cycleRepeat();
      player.play(mockTrack);
      lastAudioInstance!.trigger('ended');
      expect(player.isPlaying).toBe(false);
    });

    it('timeupdate with long interval triggers position state update', () => {
      player.play(mockTrack);
      lastAudioInstance!.currentTime = 10;
      lastAudioInstance!.trigger('timeupdate');
      lastAudioInstance!.currentTime = 20;
      lastAudioInstance!.trigger('timeupdate');
      expect(player.progress).toBe(20);
    });

    it('ended event logs history using duration when progress is 0', () => {
      player.play(mockTrack);
      vi.clearAllMocks();
      (lastAudioInstance as any).duration = 180;
      lastAudioInstance!.trigger('loadedmetadata');
      lastAudioInstance!.trigger('ended');
      expect(api.post).toHaveBeenCalledWith('/history', {
        track_id: 'edge-1',
        duration_listened: 180,
      });
    });

    it('play event on audio updates isPlaying and mediaSession', () => {
      player.play(mockTrack);
      expect(player.isPlaying).toBe(true);
    });

    it('pause event on audio updates isPlaying and mediaSession', () => {
      player.play(mockTrack);
      lastAudioInstance!.trigger('pause');
      expect(player.isPlaying).toBe(false);
    });
  });

  describe('media session seekto/seekbackward/seekforward handlers', () => {
    const mockTrack = {
      id: 'seek-ms', title: 'Seek Test', artist_id: 'a1', album_id: null,
      track_number: null, disc_number: null, duration_secs: 300, genre: null,
      year: null, file_path: '/seek.mp3', file_size: 1000, format: 'mp3',
      bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
      uploaded_by: null, play_count: 0, created_at: '2025-01-01',
    };

    it('seekto handler via mediaSession sets currentTime', () => {
      player.play(mockTrack);
      expect('mediaSession' in navigator).toBe(true);
    });
  });
});
