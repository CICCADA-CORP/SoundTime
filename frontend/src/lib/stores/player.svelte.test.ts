import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock dependencies
vi.mock('$lib/api', () => ({
  api: {
    post: vi.fn().mockResolvedValue(undefined),
    get: vi.fn().mockResolvedValue(undefined),
  },
  streamUrl: vi.fn((id: string) => `/api/tracks/${id}/stream`),
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
});
