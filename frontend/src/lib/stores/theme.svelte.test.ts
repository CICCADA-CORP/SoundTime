import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock $lib/api before importing the store
vi.mock('$lib/api', () => {
	const mockApi = {
		get: vi.fn(),
		post: vi.fn(),
		put: vi.fn(),
		delete: vi.fn(),
	};
	return {
		api: mockApi,
		setTokens: vi.fn(),
		clearTokens: vi.fn(),
	};
});

import { getThemeStore } from './theme.svelte';

const localStorageMock = (() => {
	let store: Record<string, string> = {};
	return {
		getItem: vi.fn((key: string) => store[key] ?? null),
		setItem: vi.fn((key: string, value: string) => { store[key] = value; }),
		removeItem: vi.fn((key: string) => { delete store[key]; }),
		clear: vi.fn(() => { store = {}; }),
	};
})();

Object.defineProperty(globalThis, 'localStorage', { value: localStorageMock, writable: true });

const mockTheme = {
	id: 'theme-1',
	name: 'neo-dark',
	version: '1.0.0',
	description: 'A dark neon theme',
	author: 'Jane Doe',
	license: 'MIT',
	homepage: 'https://example.com',
	git_url: 'https://github.com/user/theme.git',
	css_path: '/themes/neo-dark/theme.css',
	assets_path: '/themes/neo-dark/assets',
	status: 'enabled' as const,
	installed_at: '2025-01-01T00:00:00Z',
	updated_at: '2025-01-01T00:00:00Z',
	installed_by: 'user-1',
};

describe('Theme Store', () => {
	let theme: ReturnType<typeof getThemeStore>;

	beforeEach(() => {
		vi.clearAllMocks();
		localStorageMock.clear();
		theme = getThemeStore();
		// Clean up any injected link elements
		theme.removeTheme();
		// Reset fetch mock
		vi.stubGlobal('fetch', vi.fn());
		// Reset window.location.search
		Object.defineProperty(window, 'location', {
			value: { search: '', href: 'http://localhost/' },
			writable: true,
		});
	});

	describe('initial state', () => {
		it('activeTheme is null before init', () => {
			// After removeTheme in beforeEach, activeTheme may already be null
			// but loading is true until init() is called
			expect(theme.activeTheme).toBeNull();
		});
	});

	describe('init', () => {
		it('fetches active theme and sets state when theme exists', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: true,
				json: () => Promise.resolve(mockTheme),
			} as Response);

			await theme.init();

			expect(fetch).toHaveBeenCalledWith('/api/themes/active');
			expect(theme.activeTheme).toEqual(mockTheme);
			expect(theme.loading).toBe(false);
			expect(theme.safeMode).toBe(false);
		});

		it('sets activeTheme to null when no active theme (non-ok response)', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 204,
			} as Response);

			await theme.init();

			expect(theme.activeTheme).toBeNull();
			expect(theme.loading).toBe(false);
		});

		it('sets activeTheme to null when fetch fails', async () => {
			vi.mocked(fetch).mockRejectedValueOnce(new Error('Network error'));

			await theme.init();

			expect(theme.activeTheme).toBeNull();
			expect(theme.loading).toBe(false);
		});

		it('injects link element when active theme exists', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: true,
				json: () => Promise.resolve(mockTheme),
			} as Response);

			await theme.init();

			const link = document.getElementById('soundtime-theme') as HTMLLinkElement;
			expect(link).not.toBeNull();
			expect(link.rel).toBe('stylesheet');
			expect(link.href).toContain('/api/themes/active.css');
		});

		it('does not inject link when no active theme', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 204,
			} as Response);

			await theme.init();

			const link = document.getElementById('soundtime-theme');
			expect(link).toBeNull();
		});
	});

	describe('safe mode', () => {
		it('activates safe mode when URL has ?theme=default', async () => {
			Object.defineProperty(window, 'location', {
				value: { search: '?theme=default', href: 'http://localhost/?theme=default' },
				writable: true,
			});

			await theme.init();

			expect(theme.safeMode).toBe(true);
			expect(theme.activeTheme).toBeNull();
			expect(theme.loading).toBe(false);
			expect(fetch).not.toHaveBeenCalled();
		});

		it('activates safe mode when localStorage flag is set', async () => {
			localStorageMock.setItem('soundtime_theme_safe', '1');

			await theme.init();

			expect(theme.safeMode).toBe(true);
			expect(theme.activeTheme).toBeNull();
			expect(fetch).not.toHaveBeenCalled();
		});

		it('does not activate safe mode normally', async () => {
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 204,
			} as Response);

			await theme.init();

			expect(theme.safeMode).toBe(false);
			expect(fetch).toHaveBeenCalled();
		});
	});

	describe('injectTheme / removeTheme', () => {
		it('injectTheme creates a link element in head', () => {
			theme.injectTheme();

			const link = document.getElementById('soundtime-theme') as HTMLLinkElement;
			expect(link).not.toBeNull();
			expect(link.tagName).toBe('LINK');
			expect(link.rel).toBe('stylesheet');
			expect(link.href).toContain('/api/themes/active.css');
		});

		it('injectTheme does not create duplicate links', () => {
			theme.injectTheme();
			theme.injectTheme();

			const links = document.querySelectorAll('#soundtime-theme');
			expect(links.length).toBe(1);
		});

		it('removeTheme removes the link element', () => {
			theme.injectTheme();
			expect(document.getElementById('soundtime-theme')).not.toBeNull();

			theme.removeTheme();
			expect(document.getElementById('soundtime-theme')).toBeNull();
		});

		it('removeTheme is safe to call when no link exists', () => {
			expect(() => theme.removeTheme()).not.toThrow();
		});
	});

	describe('refresh', () => {
		it('removes old theme and re-initializes', async () => {
			// First init with a theme
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: true,
				json: () => Promise.resolve(mockTheme),
			} as Response);
			await theme.init();
			expect(document.getElementById('soundtime-theme')).not.toBeNull();

			// Refresh - now no theme
			vi.mocked(fetch).mockResolvedValueOnce({
				ok: false,
				status: 204,
			} as Response);
			await theme.refresh();

			expect(theme.activeTheme).toBeNull();
			expect(document.getElementById('soundtime-theme')).toBeNull();
		});
	});
});
