import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { clearTokens, streamUrl, API_BASE } from '$lib/api';

// Mock localStorage
const localStorageMock = (() => {
	let store: Record<string, string> = {};
	return {
		getItem: vi.fn((key: string) => store[key] ?? null),
		setItem: vi.fn((key: string, value: string) => { store[key] = value; }),
		removeItem: vi.fn((key: string) => { delete store[key]; }),
		clear: vi.fn(() => { store = {}; }),
	};
})();

Object.defineProperty(globalThis, 'localStorage', { value: localStorageMock });

describe('API module', () => {
	beforeEach(() => {
		localStorageMock.clear();
		vi.clearAllMocks();
	});

	describe('clearTokens', () => {
		it('removes access and refresh tokens from localStorage', () => {
			localStorageMock.setItem('soundtime_access_token', 'abc');
			localStorageMock.setItem('soundtime_refresh_token', 'def');
			clearTokens();
			expect(localStorageMock.removeItem).toHaveBeenCalledWith('soundtime_access_token');
			expect(localStorageMock.removeItem).toHaveBeenCalledWith('soundtime_refresh_token');
		});
	});

	describe('streamUrl', () => {
		it('returns stream URL without token when not logged in', () => {
			const url = streamUrl('track-123');
			expect(url).toBe(`${API_BASE}/tracks/track-123/stream`);
		});

		it('returns stream URL with token when logged in', () => {
			localStorageMock.setItem('soundtime_access_token', 'my-token');
			const url = streamUrl('track-456');
			expect(url).toBe(`${API_BASE}/tracks/track-456/stream?token=my-token`);
		});
	});

	describe('API_BASE', () => {
		it('has a default value', () => {
			expect(API_BASE).toBeDefined();
			expect(typeof API_BASE).toBe('string');
		});
	});
});
