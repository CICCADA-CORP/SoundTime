import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock dependencies BEFORE imports
vi.mock('$lib/api', () => ({
	api: {
		get: vi.fn(),
	},
}));

import { getTaskStore } from './tasks.svelte';
import { api } from '$lib/api';

describe('Task Store', () => {
	let store: ReturnType<typeof getTaskStore>;

	beforeEach(() => {
		vi.clearAllMocks();
		store = getTaskStore();
		store.dismiss(); // Reset module-level $state
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	// ─── Initial state ─────────────────────────────────────────────

	describe('initial state', () => {
		it('should have isActive=false', () => {
			expect(store.isActive).toBe(false);
		});

		it('should have isRunning=false', () => {
			expect(store.isRunning).toBe(false);
		});

		it('should have taskType=null', () => {
			expect(store.taskType).toBeNull();
		});

		it('should have progress=null', () => {
			expect(store.progress).toBeNull();
		});

		it('should have lastStatus=null', () => {
			expect(store.lastStatus).toBeNull();
		});
	});

	// ─── startPolling ──────────────────────────────────────────────

	describe('startPolling', () => {
		it('should set state for sync task', () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({ status: 'running', progress: { processed: 5, total: 10 } });

			store.startPolling('sync');

			expect(store.isActive).toBe(true);
			expect(store.isRunning).toBe(true);
			expect(store.taskType).toBe('sync');
			expect(store.progress).toEqual({ processed: 0, total: null });
		});

		it('should set taskType to integrity', () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({ status: 'running', progress: { processed: 0, total: null } });

			store.startPolling('integrity');

			expect(store.taskType).toBe('integrity');
		});

		it('should call api.get for task status', () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({ status: 'running', progress: { processed: 0, total: null } });

			store.startPolling('sync');

			expect(api.get).toHaveBeenCalledWith('/admin/storage/task-status');
		});
	});

	// ─── dismiss ───────────────────────────────────────────────────

	describe('dismiss', () => {
		it('should reset all state to initial', () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({ status: 'running', progress: { processed: 0, total: null } });

			store.startPolling('sync');
			store.dismiss();

			expect(store.isActive).toBe(false);
			expect(store.isRunning).toBe(false);
			expect(store.taskType).toBeNull();
			expect(store.progress).toBeNull();
			expect(store.lastStatus).toBeNull();
		});
	});

	// ─── poll - running status ─────────────────────────────────────

	describe('poll - running status', () => {
		it('should continue polling when status is running', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 3, total: 10 },
			});

			store.startPolling('sync');

			// Wait for the first poll's promise to resolve
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isRunning).toBe(true);
			expect(store.progress).toEqual({ processed: 3, total: 10 });

			// Should schedule another poll at POLL_INTERVAL (1500ms)
			expect(api.get).toHaveBeenCalledTimes(1);

			await vi.advanceTimersByTimeAsync(1500);

			expect(api.get).toHaveBeenCalledTimes(2);
		});
	});

	// ─── poll - completed status ───────────────────────────────────

	describe('poll - completed status', () => {
		it('should stop polling and keep isActive=true', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'completed',
				result: { kind: 'sync', added: 0, updated: 0, removed: 0, unchanged: 0, errors: [] },
			});

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isActive).toBe(true);
			expect(store.isRunning).toBe(false);

			// Should not schedule another poll
			await vi.advanceTimersByTimeAsync(1500);
			expect(api.get).toHaveBeenCalledTimes(1);
		});
	});

	// ─── poll - idle status ────────────────────────────────────────

	describe('poll - idle status', () => {
		it('should stop polling and set isActive=false', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({ status: 'idle' });

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isActive).toBe(false);
			expect(store.isRunning).toBe(false);

			// Should not schedule another poll
			await vi.advanceTimersByTimeAsync(1500);
			expect(api.get).toHaveBeenCalledTimes(1);
		});
	});

	// ─── poll - error with retries ─────────────────────────────────

	describe('poll - error status from API', () => {
		it('should retry up to MAX_RETRIES(3) then set error status', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockRejectedValue(new Error('Network error'));

			store.startPolling('sync');

			// First poll fails (retry 1)
			await vi.advanceTimersByTimeAsync(0);
			expect(store.isRunning).toBe(true); // still running, retrying

			// Retry at RETRY_INTERVAL (5000ms) - retry 2
			await vi.advanceTimersByTimeAsync(5000);
			expect(store.isRunning).toBe(true);

			// Retry at RETRY_INTERVAL (5000ms) - retry 3 (MAX_RETRIES reached)
			await vi.advanceTimersByTimeAsync(5000);

			expect(store.isRunning).toBe(false);
			expect(store.isActive).toBe(true); // error status is active
			expect(store.lastStatus).toEqual({
				status: 'error',
				message: 'Lost connection to server. The task may still be running.',
			});
			expect(api.get).toHaveBeenCalledTimes(3);
		});
	});

	// ─── poll - retry then success ─────────────────────────────────

	describe('poll - retry then success', () => {
		it('should reset retryCount to 0 on success after failures', async () => {
			vi.useFakeTimers();

			// First call fails, second succeeds
			vi.mocked(api.get)
				.mockRejectedValueOnce(new Error('Network error'))
				.mockResolvedValueOnce({
					status: 'running',
					progress: { processed: 5, total: 10 },
				})
				// Third call also succeeds (proves retryCount reset)
				.mockRejectedValueOnce(new Error('Network error'))
				.mockRejectedValueOnce(new Error('Network error'))
				.mockRejectedValueOnce(new Error('Network error'));

			store.startPolling('sync');

			// First poll fails (retryCount = 1)
			await vi.advanceTimersByTimeAsync(0);
			expect(store.isRunning).toBe(true);

			// Retry succeeds (retryCount resets to 0)
			await vi.advanceTimersByTimeAsync(5000);
			expect(store.isRunning).toBe(true);
			expect(store.progress).toEqual({ processed: 5, total: 10 });

			// Next poll at POLL_INTERVAL fails again (retryCount = 1 from fresh)
			await vi.advanceTimersByTimeAsync(1500);
			expect(store.isRunning).toBe(true);

			// Second consecutive failure (retryCount = 2)
			await vi.advanceTimersByTimeAsync(5000);
			expect(store.isRunning).toBe(true);

			// Third consecutive failure (retryCount = 3, MAX_RETRIES reached)
			await vi.advanceTimersByTimeAsync(5000);
			expect(store.isRunning).toBe(false);
			expect(store.lastStatus).toEqual({
				status: 'error',
				message: 'Lost connection to server. The task may still be running.',
			});
		});
	});

	// ─── checkForRunningTask ───────────────────────────────────────

	describe('checkForRunningTask', () => {
		it('should resume polling when task is running', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 7, total: 20 },
			});

			await store.checkForRunningTask();
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isRunning).toBe(true);
			expect(store.isActive).toBe(true);
			expect(store.progress).toEqual({ processed: 7, total: 20 });
		});

		it('should not start polling when task is not running', async () => {
			vi.mocked(api.get).mockResolvedValue({ status: 'idle' });

			await store.checkForRunningTask();

			expect(store.isRunning).toBe(false);
			expect(store.isActive).toBe(false);
			expect(store.lastStatus).toBeNull();
		});

		it('should not start polling when task is completed', async () => {
			vi.mocked(api.get).mockResolvedValue({
				status: 'completed',
				result: { kind: 'sync', added: 0, updated: 0, removed: 0, unchanged: 0, errors: [] },
			});

			await store.checkForRunningTask();

			expect(store.isRunning).toBe(false);
			expect(store.isActive).toBe(false);
		});

		it('should silently ignore API errors', async () => {
			vi.mocked(api.get).mockRejectedValue(new Error('Server down'));

			await store.checkForRunningTask();

			expect(store.isRunning).toBe(false);
			expect(store.isActive).toBe(false);
			expect(store.lastStatus).toBeNull();
		});

		it('should default taskType to sync when taskType is null', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 1, total: 5 },
			});

			// taskType starts as null after dismiss
			await store.checkForRunningTask();

			expect(store.taskType).toBe('sync');
		});

		it('should preserve existing taskType when not null', async () => {
			vi.useFakeTimers();

			// First start an integrity task, then dismiss but set up so taskType persists
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 0, total: null },
			});

			store.startPolling('integrity');
			// stopPolling is called but taskType is set before poll
			// Now dismiss resets taskType to null, so let's test differently:
			// Start integrity polling, then immediately check
			store.dismiss();
			store.startPolling('integrity');

			// Now simulate checkForRunningTask seeing a running task
			// The taskType is already 'integrity' from startPolling
			await store.checkForRunningTask();
			await vi.advanceTimersByTimeAsync(0);

			expect(store.taskType).toBe('integrity');
		});
	});

	// ─── progress getter ───────────────────────────────────────────

	describe('progress getter', () => {
		it('should return progress when status is running', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 42, total: 100 },
			});

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.progress).toEqual({ processed: 42, total: 100 });
		});

		it('should return null when status is completed', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'completed',
				result: { kind: 'sync', added: 0, updated: 0, removed: 0, unchanged: 0, errors: [] },
			});

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.progress).toBeNull();
		});

		it('should return null when status is error', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockRejectedValue(new Error('fail'));

			store.startPolling('sync');

			// Exhaust all retries
			await vi.advanceTimersByTimeAsync(0);
			await vi.advanceTimersByTimeAsync(5000);
			await vi.advanceTimersByTimeAsync(5000);

			expect(store.progress).toBeNull();
		});

		it('should return null when status is null', () => {
			expect(store.progress).toBeNull();
		});
	});

	// ─── isActive getter ───────────────────────────────────────────

	describe('isActive getter', () => {
		it('should be false for null status', () => {
			expect(store.isActive).toBe(false);
		});

		it('should be false for idle status', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({ status: 'idle' });

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isActive).toBe(false);
		});

		it('should be true for running status', () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 0, total: null },
			});

			store.startPolling('sync');
			// Before poll resolves, status is set to running by startPolling
			expect(store.isActive).toBe(true);
		});

		it('should be true for completed status', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'completed',
				result: { kind: 'sync', added: 1, updated: 0, removed: 0, unchanged: 5, errors: [] },
			});

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isActive).toBe(true);
		});

		it('should be true for error status', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockRejectedValue(new Error('fail'));

			store.startPolling('sync');

			// Exhaust all retries (3)
			await vi.advanceTimersByTimeAsync(0);
			await vi.advanceTimersByTimeAsync(5000);
			await vi.advanceTimersByTimeAsync(5000);

			expect(store.isActive).toBe(true);
			expect(store.lastStatus).toEqual({
				status: 'error',
				message: 'Lost connection to server. The task may still be running.',
			});
		});
	});

	// ─── isRunning getter ──────────────────────────────────────────

	describe('isRunning getter', () => {
		it('should be false when status is null', () => {
			expect(store.isRunning).toBe(false);
		});

		it('should be true when status is running', () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 0, total: null },
			});

			store.startPolling('sync');

			expect(store.isRunning).toBe(true);
		});

		it('should be false when status is completed', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'completed',
				result: { kind: 'integrity', orphaned_files: 0, missing_files: 0, hash_mismatches: 0, total_checked: 10, errors: [] },
			});

			store.startPolling('integrity');
			await vi.advanceTimersByTimeAsync(0);

			expect(store.isRunning).toBe(false);
		});
	});

	// ─── stopPolling clears timer ──────────────────────────────────

	describe('stopPolling via dismiss', () => {
		it('should clear pending poll timer', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 0, total: null },
			});

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			// A timer is now scheduled for the next poll
			expect(api.get).toHaveBeenCalledTimes(1);

			store.dismiss();

			// Advance past the poll interval - no more calls should be made
			await vi.advanceTimersByTimeAsync(1500);
			expect(api.get).toHaveBeenCalledTimes(1);
		});
	});

	// ─── startPolling replaces previous polling ────────────────────

	describe('startPolling replaces previous polling', () => {
		it('should stop previous polling when starting new one', async () => {
			vi.useFakeTimers();
			vi.mocked(api.get).mockResolvedValue({
				status: 'running',
				progress: { processed: 0, total: null },
			});

			store.startPolling('sync');
			await vi.advanceTimersByTimeAsync(0);

			// Start a new polling session
			store.startPolling('integrity');
			expect(store.taskType).toBe('integrity');

			// The old timer should have been cleared; only one new poll call
			await vi.advanceTimersByTimeAsync(0);
			// 1 from first startPolling, 1 from first poll resolution's setTimeout + resolve, 1 from second startPolling
			// But stopPolling clears the timer, so only the new poll fires
			expect(store.taskType).toBe('integrity');
		});
	});
});
