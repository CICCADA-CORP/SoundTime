import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock dependencies BEFORE imports
const mockQueueStore = {
	queue: [] as any[],
	currentIndex: -1,
	currentTrack: null,
	hasNext: false,
	hasPrevious: false,
	radioMode: false,
	playQueue: vi.fn(),
	addToQueue: vi.fn(),
	addNext: vi.fn(),
	removeFromQueue: vi.fn(),
	clearQueue: vi.fn(),
	next: vi.fn(),
	previous: vi.fn(),
};

vi.mock('./queue.svelte', () => ({
	getQueueStore: () => mockQueueStore,
}));

vi.mock('$lib/api', () => ({
	radioApi: {
		next: vi.fn(),
	},
}));

import { getRadioStore } from './radio.svelte';
import { radioApi } from '$lib/api';

const mockTrack = (id: string) => ({
	id,
	title: `Track ${id}`,
	artist_id: 'artist-1',
	album_id: null,
	track_number: null,
	disc_number: null,
	duration_secs: 200,
	genre: 'Rock',
	year: 2020,
	file_path: `/tracks/${id}.mp3`,
	file_size: 5000000,
	format: 'mp3',
	bitrate: 320,
	sample_rate: 44100,
	musicbrainz_id: null,
	waveform_data: null,
	uploaded_by: null,
	play_count: 10,
	created_at: '2024-01-01T00:00:00Z',
	artist_name: 'Test Artist',
	album_title: 'Test Album',
	cover_url: undefined,
});

describe('Radio Store', () => {
	let radio: ReturnType<typeof getRadioStore>;

	beforeEach(() => {
		vi.clearAllMocks();
		radio = getRadioStore();
		radio.stopRadio(); // Reset module-level $state
	});

	describe('initial state', () => {
		it('should be inactive by default', () => {
			expect(radio.active).toBe(false);
			expect(radio.seedType).toBeNull();
			expect(radio.seedLabel).toBe('');
			expect(radio.loading).toBe(false);
			expect(radio.exhausted).toBe(false);
			expect(radio.playedCount).toBe(0);
			expect(radio.error).toBeNull();
		});
	});

	describe('startRadio', () => {
		it('should start radio and set active state', async () => {
			const tracks = [mockTrack('t1'), mockTrack('t2'), mockTrack('t3')];
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks,
				exhausted: false,
			});

			await radio.startRadio('track', { seedId: 't1', label: 'Test Track' });

			expect(radio.active).toBe(true);
			expect(radio.seedType).toBe('track');
			expect(radio.seedLabel).toBe('Test Track');
			expect(radio.loading).toBe(false);
			expect(radio.exhausted).toBe(false);
			expect(radio.playedCount).toBe(3);
			expect(mockQueueStore.playQueue).toHaveBeenCalledWith(tracks, 0);
		});

		it('should call radioApi.next with correct params for track seed', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});

			await radio.startRadio('track', { seedId: 'seed-123', label: 'Song' });

			expect(radioApi.next).toHaveBeenCalledWith({
				seed_type: 'track',
				seed_id: 'seed-123',
				genre: undefined,
				count: 5,
				exclude: [],
			});
		});

		it('should call radioApi.next with correct params for genre seed', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});

			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			expect(radioApi.next).toHaveBeenCalledWith({
				seed_type: 'genre',
				seed_id: undefined,
				genre: 'Rock',
				count: 5,
				exclude: [],
			});
		});

		it('should set exhausted and deactivate when no tracks returned', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [],
				exhausted: true,
			});

			await radio.startRadio('genre', { genre: 'Polka', label: 'Polka' });

			expect(radio.active).toBe(false);
			expect(radio.exhausted).toBe(true);
		});

		it('should set active to false on API error', async () => {
			vi.mocked(radioApi.next).mockRejectedValueOnce(new Error('Network error'));

			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			expect(radio.active).toBe(false);
			expect(radio.loading).toBe(false);
			expect(radio.error).toBe('Network error');
		});

		it('should reset state on new radio start', async () => {
			// Start first radio
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1'), mockTrack('t2')],
				exhausted: false,
			});
			await radio.startRadio('track', { seedId: 't1', label: 'First' });
			expect(radio.playedCount).toBe(2);

			// Start second radio - state should be reset
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t3')],
				exhausted: false,
			});
			await radio.startRadio('artist', { seedId: 'a1', label: 'Second' });

			expect(radio.seedType).toBe('artist');
			expect(radio.seedLabel).toBe('Second');
			expect(radio.playedCount).toBe(1); // Only t3, previous IDs cleared
		});
	});

	describe('stopRadio', () => {
		it('should stop radio and preserve queue', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('track', { seedId: 't1', label: 'Test' });
			expect(radio.active).toBe(true);

			radio.stopRadio();

			expect(radio.active).toBe(false);
			expect(radio.seedType).toBeNull();
			expect(radio.seedLabel).toBe('');
			expect(radio.exhausted).toBe(false);
			// Queue should NOT be cleared (no call to clearQueue)
			expect(mockQueueStore.clearQueue).not.toHaveBeenCalled();
		});
	});

	describe('markPlayed', () => {
		it('should add track ID to played set', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			const countBefore = radio.playedCount;
			radio.markPlayed('t99');
			expect(radio.playedCount).toBe(countBefore + 1);
		});

		it('should not duplicate IDs', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			radio.markPlayed('t1'); // Already in played set from startRadio
			expect(radio.playedCount).toBe(1); // Should still be 1
		});
	});

	describe('fetchMoreTracks', () => {
		it('should fetch more tracks and append to queue', async () => {
			// Start radio first
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			// Fetch more
			const newTracks = [mockTrack('t2'), mockTrack('t3')];
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: newTracks,
				exhausted: false,
			});
			await radio.fetchMoreTracks();

			expect(mockQueueStore.addToQueue).toHaveBeenCalledTimes(2);
			expect(mockQueueStore.addToQueue).toHaveBeenCalledWith(newTracks[0]);
			expect(mockQueueStore.addToQueue).toHaveBeenCalledWith(newTracks[1]);
			expect(radio.playedCount).toBe(3); // t1 + t2 + t3
		});

		it('should not fetch when exhausted', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: true,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });
			expect(radio.exhausted).toBe(true);

			vi.clearAllMocks();
			await radio.fetchMoreTracks();

			expect(radioApi.next).not.toHaveBeenCalled();
		});

		it('should not fetch when radio is inactive', async () => {
			await radio.fetchMoreTracks();
			expect(radioApi.next).not.toHaveBeenCalled();
		});

		it('should set exhausted when no more tracks returned', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [],
				exhausted: true,
			});
			await radio.fetchMoreTracks();

			expect(radio.exhausted).toBe(true);
		});

		it('should send exclude list with played IDs', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1'), mockTrack('t2')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t3')],
				exhausted: false,
			});
			await radio.fetchMoreTracks();

			const lastCall = vi.mocked(radioApi.next).mock.calls[1][0];
			expect(lastCall.exclude).toContain('t1');
			expect(lastCall.exclude).toContain('t2');
		});

		it('should handle API errors silently', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			vi.mocked(radioApi.next).mockRejectedValueOnce(new Error('Network error'));
			await radio.fetchMoreTracks();

			// Should not throw, radio stays active
			expect(radio.active).toBe(true);
			expect(radio.loading).toBe(false);
		});

		it('resets playedIds and retries when tracks empty but not server-exhausted', async () => {
			// Start radio with played tracks
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1'), mockTrack('t2')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });
			expect(radio.playedCount).toBe(2);

			// fetchMoreTracks returns empty but server says NOT exhausted
			vi.mocked(radioApi.next)
				.mockResolvedValueOnce({ tracks: [], exhausted: false })
				// The retry call should succeed with tracks
				.mockResolvedValueOnce({ tracks: [mockTrack('t3')], exhausted: false });

			await radio.fetchMoreTracks();

			// Should have retried and gotten t3
			expect(radio.exhausted).toBe(false);
			// radioApi.next called: 1 (start) + 1 (empty) + 1 (retry) = 3
			expect(radioApi.next).toHaveBeenCalledTimes(3);
		});

		it('does not retry when already retrying (retrying=true)', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			// Return empty, not exhausted — but since we can't directly call with retrying=true,
			// we simulate double-empty: first call resets and retries, retry also returns empty
			vi.mocked(radioApi.next)
				.mockResolvedValueOnce({ tracks: [], exhausted: false })
				.mockResolvedValueOnce({ tracks: [], exhausted: true });

			await radio.fetchMoreTracks();
			expect(radio.exhausted).toBe(true);
		});

		it('resets playedIds when exhausted with tracks and not retrying', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			// Server returns tracks but signals exhaustion
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t2')],
				exhausted: true,
			});
			await radio.fetchMoreTracks();

			// Should have reset playedIds and exhausted for next call
			expect(radio.exhausted).toBe(false);
			// playedCount should be 1 (only t2 re-added after reset)
			expect(radio.playedCount).toBe(1);
		});

		it('limits exclude list to 2000 IDs', async () => {
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			// Manually mark 2500 tracks as played
			for (let i = 0; i < 2500; i++) {
				radio.markPlayed(`track-${i}`);
			}

			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('new1')],
				exhausted: false,
			});
			await radio.fetchMoreTracks();

			const lastCall = vi.mocked(radioApi.next).mock.calls[1][0];
			expect(lastCall.exclude.length).toBeLessThanOrEqual(2000);
		});
	});

	describe('error state', () => {
		it('should have null error by default', () => {
			expect(radio.error).toBeNull();
		});

		it('should set error on API failure in startRadio', async () => {
			vi.mocked(radioApi.next).mockRejectedValueOnce(new Error('Network error'));

			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			expect(radio.error).toBe('Network error');
			expect(radio.active).toBe(false);
		});

		it('should set default error message for non-Error objects', async () => {
			vi.mocked(radioApi.next).mockRejectedValueOnce('string error');

			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });

			expect(radio.error).toBe('Failed to start radio');
		});

		it('should clear error on new startRadio call', async () => {
			// First call fails
			vi.mocked(radioApi.next).mockRejectedValueOnce(new Error('Network error'));
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });
			expect(radio.error).toBe('Network error');

			// Second call succeeds - error should be cleared
			vi.mocked(radioApi.next).mockResolvedValueOnce({
				tracks: [mockTrack('t1')],
				exhausted: false,
			});
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });
			expect(radio.error).toBeNull();
		});

		it('should clear error on stopRadio', async () => {
			vi.mocked(radioApi.next).mockRejectedValueOnce(new Error('Network error'));
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });
			expect(radio.error).toBe('Network error');

			radio.stopRadio();
			expect(radio.error).toBeNull();
		});

		it('should clear error via clearError()', async () => {
			vi.mocked(radioApi.next).mockRejectedValueOnce(new Error('Network error'));
			await radio.startRadio('genre', { genre: 'Rock', label: 'Rock' });
			expect(radio.error).toBe('Network error');

			radio.clearError();
			expect(radio.error).toBeNull();
		});
	});
});
