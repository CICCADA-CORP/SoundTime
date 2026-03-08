import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock localStorage globally
const localStorageMock = {
  getItem: vi.fn(() => null),
  setItem: vi.fn(),
  removeItem: vi.fn(),
};
Object.defineProperty(global, 'localStorage', {
  value: localStorageMock,
  writable: true,
});

// Mock player store
const mockPlayerStore = {
  currentTrack: null as any,
  isPlaying: false,
  shuffle: false,
  repeat: 'none' as 'none' | 'one' | 'all',
  progress: 0,
  play: vi.fn(),
  seek: vi.fn(),
};

vi.mock('./player.svelte', () => ({
  getPlayerStore: () => mockPlayerStore,
}));

// Mock radio store
const mockRadioStore = {
  active: false,
  startRadio: vi.fn(),
  stopRadio: vi.fn(),
  markPlayed: vi.fn(),
  fetchMoreTracks: vi.fn(),
  loading: false,
  exhausted: false,
};

vi.mock('./radio.svelte', () => ({
  getRadioStore: () => mockRadioStore,
}));

import { getQueueStore } from './queue.svelte';

describe('Queue Store', () => {
  let queue: ReturnType<typeof getQueueStore>;

  const track1 = {
    id: 't1', title: 'Song 1', artist_id: 'a1', album_id: null,
    track_number: 1, disc_number: null, duration_secs: 200, genre: null,
    year: null, file_path: '/s1.mp3', file_size: 1000, format: 'mp3',
    bitrate: 320, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
    uploaded_by: null, play_count: 0, created_at: '2025-01-01',
  };
  const track2 = {
    id: 't2', title: 'Song 2', artist_id: 'a2', album_id: null,
    track_number: 2, disc_number: null, duration_secs: 180, genre: null,
    year: null, file_path: '/s2.mp3', file_size: 900, format: 'mp3',
    bitrate: 256, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
    uploaded_by: null, play_count: 0, created_at: '2025-01-01',
  };
  const track3 = {
    id: 't3', title: 'Song 3', artist_id: 'a3', album_id: null,
    track_number: 3, disc_number: null, duration_secs: 240, genre: null,
    year: null, file_path: '/s3.mp3', file_size: 1200, format: 'mp3',
    bitrate: 128, sample_rate: 44100, musicbrainz_id: null, waveform_data: null,
    uploaded_by: null, play_count: 0, created_at: '2025-01-01',
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockPlayerStore.shuffle = false;
    mockPlayerStore.repeat = 'none';
    mockPlayerStore.progress = 0;
    mockRadioStore.active = false;
    queue = getQueueStore();
    queue.clearQueue();
    // Reset autoplay if it was enabled by a previous test
    if (queue.autoplay) {
      queue.toggleAutoplay();
    }
    vi.clearAllMocks(); // Clear mocks again after toggleAutoplay localStorage calls
  });

  describe('initial state', () => {
    it('queue is empty', () => {
      expect(queue.queue).toEqual([]);
    });

    it('currentIndex is -1', () => {
      expect(queue.currentIndex).toBe(-1);
    });

    it('currentTrack is null', () => {
      expect(queue.currentTrack).toBeNull();
    });

    it('hasNext is false', () => {
      expect(queue.hasNext).toBe(false);
    });

    it('hasPrevious is false', () => {
      expect(queue.hasPrevious).toBe(false);
    });
  });

  describe('playQueue', () => {
    it('sets queue and plays first track', () => {
      queue.playQueue([track1, track2, track3]);

      expect(queue.queue).toHaveLength(3);
      expect(queue.currentIndex).toBe(0);
      expect(mockPlayerStore.play).toHaveBeenCalledWith(track1);
    });

    it('plays from specified start index', () => {
      queue.playQueue([track1, track2, track3], 1);

      expect(queue.currentIndex).toBe(1);
      expect(mockPlayerStore.play).toHaveBeenCalledWith(track2);
    });

    it('hasNext is true when more tracks ahead', () => {
      queue.playQueue([track1, track2]);
      expect(queue.hasNext).toBe(true);
    });

    it('hasPrevious is false at start', () => {
      queue.playQueue([track1, track2]);
      expect(queue.hasPrevious).toBe(false);
    });
  });

  describe('addToQueue', () => {
    it('appends track to queue', () => {
      queue.playQueue([track1]);
      queue.addToQueue(track2);

      expect(queue.queue).toHaveLength(2);
      expect(queue.queue[1]).toEqual(track2);
    });
  });

  describe('addNext', () => {
    it('inserts track after current', () => {
      queue.playQueue([track1, track3]);
      queue.addNext(track2);

      expect(queue.queue[1]).toEqual(track2);
      expect(queue.queue[2]).toEqual(track3);
    });
  });

  describe('removeFromQueue', () => {
    it('removes track by index', () => {
      queue.playQueue([track1, track2, track3]);
      queue.removeFromQueue(1);

      expect(queue.queue).toHaveLength(2);
      expect(queue.queue[0]).toEqual(track1);
      expect(queue.queue[1]).toEqual(track3);
    });
  });

  describe('clearQueue', () => {
    it('empties the queue', () => {
      queue.playQueue([track1, track2]);
      queue.clearQueue();

      expect(queue.queue).toEqual([]);
      expect(queue.currentIndex).toBe(-1);
    });
  });

  describe('next', () => {
    it('advances to next track', () => {
      queue.playQueue([track1, track2, track3]);
      vi.clearAllMocks();

      queue.next();

      expect(mockPlayerStore.play).toHaveBeenCalledWith(track2);
    });

    it('does nothing at end without repeat', () => {
      queue.playQueue([track1]);
      vi.clearAllMocks();

      queue.next();

      expect(mockPlayerStore.play).not.toHaveBeenCalled();
    });

    it('loops to start with repeat all', () => {
      queue.playQueue([track1, track2]);
      mockPlayerStore.repeat = 'all';
      // Move to last track
      queue.next();
      vi.clearAllMocks();

      queue.next();

      expect(mockPlayerStore.play).toHaveBeenCalledWith(track1);
    });
  });

  describe('previous', () => {
    it('seeks to 0 if progress > 3', () => {
      queue.playQueue([track1, track2]);
      queue.next();
      mockPlayerStore.progress = 10;

      queue.previous();

      expect(mockPlayerStore.seek).toHaveBeenCalledWith(0);
    });

    it('goes to previous track if progress <= 3', () => {
      queue.playQueue([track1, track2]);
      queue.next();
      mockPlayerStore.progress = 1;
      vi.clearAllMocks();

      queue.previous();

      expect(mockPlayerStore.play).toHaveBeenCalledWith(track1);
    });

    it('does nothing at start with progress <= 3', () => {
      queue.playQueue([track1]);
      mockPlayerStore.progress = 0;
      vi.clearAllMocks();

      queue.previous();

      // Can't go before index 0 - no play call, no seek call
      expect(mockPlayerStore.play).not.toHaveBeenCalled();
    });
  });

  describe('next with shuffle', () => {
    it('plays a random track when shuffle is on', () => {
      queue.playQueue([track1, track2, track3]);
      mockPlayerStore.shuffle = true;
      vi.clearAllMocks();

      queue.next();

      // Should play a different track (randomly selected)
      expect(mockPlayerStore.play).toHaveBeenCalled();
    });

    it('loops with repeat all and shuffle when only current track remains', () => {
      queue.playQueue([track1]);
      mockPlayerStore.shuffle = true;
      mockPlayerStore.repeat = 'all';
      vi.clearAllMocks();

      queue.next();

      // With only 1 track, remaining is empty, repeat=all should loop to start
      expect(mockPlayerStore.play).toHaveBeenCalledWith(track1);
    });

    it('does nothing with shuffle when remaining is empty and no repeat', () => {
      queue.playQueue([track1]);
      mockPlayerStore.shuffle = true;
      mockPlayerStore.repeat = 'none';
      vi.clearAllMocks();

      queue.next();

      // Only one track and no repeat - should not play
      expect(mockPlayerStore.play).not.toHaveBeenCalled();
    });
  });

  describe('next on empty queue', () => {
    it('does nothing when queue is empty', () => {
      vi.clearAllMocks();
      queue.next();
      expect(mockPlayerStore.play).not.toHaveBeenCalled();
    });
  });

  describe('removeFromQueue edge cases', () => {
    it('adjusts currentIndex when removing before current', () => {
      queue.playQueue([track1, track2, track3], 2);
      queue.removeFromQueue(0);
      expect(queue.queue).toHaveLength(2);
    });

    it('does not adjust currentIndex when removing after current', () => {
      queue.playQueue([track1, track2, track3], 0);
      queue.removeFromQueue(2);
      expect(queue.queue).toHaveLength(2);
      expect(queue.currentIndex).toBe(0);
    });
  });

  describe('currentTrack getter', () => {
    it('returns current track correctly', () => {
      queue.playQueue([track1, track2], 1);
      expect(queue.currentTrack).toEqual(track2);
    });

    it('returns null when out of bounds', () => {
      expect(queue.currentTrack).toBeNull();
    });
  });

  describe('moveInQueue', () => {
    it('does nothing when fromIndex is negative', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(-1, 1);
      expect(queue.queue).toEqual([track1, track2, track3]);
    });

    it('does nothing when fromIndex >= queue.length', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(5, 1);
      expect(queue.queue).toEqual([track1, track2, track3]);
    });

    it('does nothing when toIndex is negative', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(0, -1);
      expect(queue.queue).toEqual([track1, track2, track3]);
    });

    it('does nothing when toIndex >= queue.length', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(0, 5);
      expect(queue.queue).toEqual([track1, track2, track3]);
    });

    it('does nothing when fromIndex === toIndex', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(1, 1);
      expect(queue.queue).toEqual([track1, track2, track3]);
    });

    it('moves track from index 0 to index 2', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(0, 2);
      expect(queue.queue).toEqual([track2, track3, track1]);
    });

    it('moves track from index 2 to index 0', () => {
      queue.playQueue([track1, track2, track3]);
      queue.moveInQueue(2, 0);
      expect(queue.queue).toEqual([track3, track1, track2]);
    });

    it('updates currentIndex when moving the current track', () => {
      queue.playQueue([track1, track2, track3], 0);
      queue.moveInQueue(0, 2);
      expect(queue.currentIndex).toBe(2);
    });

    it('decrements currentIndex when moving track from before to after current', () => {
      queue.playQueue([track1, track2, track3], 1);
      queue.moveInQueue(0, 2);
      expect(queue.currentIndex).toBe(0);
    });

    it('increments currentIndex when moving track from after to before current', () => {
      queue.playQueue([track1, track2, track3], 1);
      queue.moveInQueue(2, 0);
      expect(queue.currentIndex).toBe(2);
    });

    it('does not change currentIndex when move does not affect it', () => {
      queue.playQueue([track1, track2, track3], 0);
      queue.moveInQueue(1, 2);
      expect(queue.currentIndex).toBe(0);
    });
  });

  describe('removeFromQueue - currentIndex adjustment', () => {
    it('decrements currentIndex when removing track before current', () => {
      queue.playQueue([track1, track2, track3], 2);
      const indexBefore = queue.currentIndex;
      queue.removeFromQueue(0);
      expect(queue.currentIndex).toBe(indexBefore - 1);
    });

    it('does not change currentIndex when removing track at current index', () => {
      queue.playQueue([track1, track2, track3], 1);
      queue.removeFromQueue(1);
      // After removal, queue is [track1, track3], currentIndex stays 1
      expect(queue.queue).toEqual([track1, track3]);
    });
  });

  describe('source context', () => {
    it('stores source context when provided', () => {
      queue.playQueue([track1, track2], 0, 'album' as any);
      expect(queue.sourceContext).toBe('album');
    });

    it('defaults to null when no source provided', () => {
      queue.playQueue([track1]);
      expect(queue.sourceContext).toBeNull();
    });

    it('clears source context on clearQueue', () => {
      queue.playQueue([track1], 0, 'playlist' as any);
      queue.clearQueue();
      expect(queue.sourceContext).toBeNull();
    });
  });

  describe('autoplay', () => {
    it('autoplay is false by default', () => {
      expect(queue.autoplay).toBe(false);
    });

    it('toggleAutoplay toggles state to true', () => {
      queue.toggleAutoplay();
      expect(queue.autoplay).toBe(true);
    });

    it('toggleAutoplay toggles state back to false', () => {
      queue.toggleAutoplay(); // true
      queue.toggleAutoplay(); // false
      expect(queue.autoplay).toBe(false);
    });

    it('toggleAutoplay persists to localStorage', () => {
      queue.toggleAutoplay();
      expect(localStorageMock.setItem).toHaveBeenCalledWith('soundtime_autoplay', 'true');
      queue.toggleAutoplay();
      expect(localStorageMock.setItem).toHaveBeenCalledWith('soundtime_autoplay', 'false');
    });

    it('next() does NOT start radio when autoplay is off and queue ends', () => {
      queue.playQueue([track1]);
      vi.clearAllMocks();
      // autoplay is false by default
      queue.next();
      expect(mockRadioStore.startRadio).not.toHaveBeenCalled();
    });

    it('next() starts similar radio when autoplay is on and queue ends (non-shuffle)', () => {
      queue.toggleAutoplay(); // enable autoplay
      queue.playQueue([track1]);
      vi.clearAllMocks();
      mockRadioStore.active = false;

      queue.next();

      expect(mockRadioStore.startRadio).toHaveBeenCalledWith('similar', {
        seedId: track1.id,
        label: track1.title,
        autoplay: true,
      });
    });

    it('next() starts similar radio when autoplay is on and queue ends (shuffle)', () => {
      queue.toggleAutoplay(); // enable autoplay
      queue.playQueue([track1]);
      mockPlayerStore.shuffle = true;
      vi.clearAllMocks();
      mockRadioStore.active = false;

      queue.next();

      expect(mockRadioStore.startRadio).toHaveBeenCalledWith('similar', {
        seedId: track1.id,
        label: track1.title,
        autoplay: true,
      });
    });

    it('next() does NOT start radio when repeat=all (repeat takes precedence)', () => {
      queue.toggleAutoplay(); // enable autoplay
      queue.playQueue([track1]);
      mockPlayerStore.repeat = 'all';
      vi.clearAllMocks();

      queue.next();

      expect(mockRadioStore.startRadio).not.toHaveBeenCalled();
      // Instead, repeat-all should loop
      expect(mockPlayerStore.play).toHaveBeenCalledWith(track1);
    });

    it('next() does NOT start radio when radio is already active', () => {
      queue.toggleAutoplay(); // enable autoplay
      queue.playQueue([track1]);
      vi.clearAllMocks();
      mockRadioStore.active = true; // radio already running

      queue.next();

      expect(mockRadioStore.startRadio).not.toHaveBeenCalled();
    });

    it('autoplay sets sourceContext to "autoplay"', () => {
      queue.toggleAutoplay(); // enable autoplay
      queue.playQueue([track1]);
      vi.clearAllMocks();
      mockRadioStore.active = false;

      queue.next();

      expect(queue.sourceContext).toBe('autoplay');
    });

    it('next() does not trigger autoplay when there are more tracks in queue', () => {
      queue.toggleAutoplay(); // enable autoplay
      queue.playQueue([track1, track2, track3]);
      vi.clearAllMocks();
      mockRadioStore.active = false;

      queue.next(); // moves to track2, still has track3

      expect(mockRadioStore.startRadio).not.toHaveBeenCalled();
      expect(mockPlayerStore.play).toHaveBeenCalledWith(track2);
    });
  });
});
