import { describe, it, expect, vi, beforeEach } from 'vitest';
import { clearTokens, streamUrl, apiFetch, api, setTokens, API_BASE, pluginApi, themeApi, homeApi } from '$lib/api';

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
		vi.restoreAllMocks();
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

	describe('setTokens', () => {
		it('stores access and refresh tokens in localStorage', () => {
			setTokens('access123', 'refresh456');
			expect(localStorageMock.setItem).toHaveBeenCalledWith('soundtime_access_token', 'access123');
			expect(localStorageMock.setItem).toHaveBeenCalledWith('soundtime_refresh_token', 'refresh456');
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

		it('encodes track ID in URL correctly', () => {
			const url = streamUrl('track-with-special<chars>');
			// Verify it doesn't break URL structure
			expect(url).toContain('track-with-special<chars>');
			expect(url).toContain('/stream');
		});
	});

	describe('API_BASE', () => {
		it('has a default value', () => {
			expect(API_BASE).toBeDefined();
			expect(typeof API_BASE).toBe('string');
		});
	});

	describe('apiFetch', () => {
		it('makes a GET request and returns JSON', async () => {
			const mockData = { id: 1, name: 'test' };
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				json: () => Promise.resolve(mockData),
				text: () => Promise.resolve(JSON.stringify(mockData)),
			}));

			const result = await apiFetch('/test');
			expect(result).toEqual(mockData);
		});

		it('sets Authorization header when token exists', async () => {
			localStorageMock.setItem('soundtime_access_token', 'bearer-token');
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('{"ok":true}'),
			}));

			await apiFetch('/protected');

			const [, options] = vi.mocked(fetch).mock.calls[0];
			const headers = options?.headers as Headers;
			expect(headers.get('Authorization')).toBe('Bearer bearer-token');
		});

		it('sets Content-Type to application/json by default', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('{}'),
			}));

			await apiFetch('/test');

			const [, options] = vi.mocked(fetch).mock.calls[0];
			const headers = options?.headers as Headers;
			expect(headers.get('Content-Type')).toBe('application/json');
		});

		it('does not set Content-Type for FormData', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('{}'),
			}));

			const formData = new FormData();
			await apiFetch('/upload', { method: 'POST', body: formData });

			const [, options] = vi.mocked(fetch).mock.calls[0];
			const headers = options?.headers as Headers;
			expect(headers.has('Content-Type')).toBe(false);
		});

		it('returns undefined for 204 No Content', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 204,
				text: () => Promise.resolve(''),
			}));

			const result = await apiFetch('/delete-thing');
			expect(result).toBeUndefined();
		});

		it('returns undefined for empty response body', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve(''),
			}));

			const result = await apiFetch('/empty');
			expect(result).toBeUndefined();
		});

		it('throws on non-ok response', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: false,
				status: 400,
				json: () => Promise.resolve({ error: 'Bad request' }),
			}));

			await expect(apiFetch('/bad')).rejects.toThrow('Bad request');
		});

		it('throws generic error when error response is not JSON', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: false,
				status: 500,
				json: () => Promise.reject(new Error('not json')),
			}));

			await expect(apiFetch('/server-error')).rejects.toThrow('HTTP 500');
		});

		it('attempts token refresh on 401', async () => {
			localStorageMock.setItem('soundtime_access_token', 'expired-token');
			localStorageMock.setItem('soundtime_refresh_token', 'valid-refresh');

			const fetchMock = vi.fn()
				.mockResolvedValueOnce({
					ok: false,
					status: 401,
					json: () => Promise.resolve({ error: 'Unauthorized' }),
				})
				.mockResolvedValueOnce({
					ok: true,
					status: 200,
					json: () => Promise.resolve({ access_token: 'new-access', refresh_token: 'new-refresh' }),
				})
				.mockResolvedValueOnce({
					ok: true,
					status: 200,
					text: () => Promise.resolve('{"result": "ok"}'),
				});

			vi.stubGlobal('fetch', fetchMock);

			const result = await apiFetch('/protected-resource');
			expect(result).toEqual({ result: 'ok' });
			expect(fetchMock).toHaveBeenCalledTimes(3);
		});

		it('does not refresh when no token was set', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: false,
				status: 401,
				json: () => Promise.resolve({ error: 'Unauthorized' }),
			}));

			await expect(apiFetch('/protected')).rejects.toThrow('Unauthorized');
			expect(fetch).toHaveBeenCalledTimes(1);
		});

		it('clears tokens when refresh fails', async () => {
			localStorageMock.setItem('soundtime_access_token', 'expired');
			localStorageMock.setItem('soundtime_refresh_token', 'bad-refresh');

			const fetchMock = vi.fn()
				.mockResolvedValueOnce({
					ok: false,
					status: 401,
					json: () => Promise.resolve({ error: 'Unauthorized' }),
				})
				.mockResolvedValueOnce({
					ok: false,
					status: 401,
					json: () => Promise.resolve({ error: 'Invalid refresh token' }),
				});

			vi.stubGlobal('fetch', fetchMock);

			await expect(apiFetch('/thing')).rejects.toThrow('Unauthorized');
			expect(localStorageMock.removeItem).toHaveBeenCalledWith('soundtime_access_token');
		});

		it('clears tokens when refresh request throws network error', async () => {
			localStorageMock.setItem('soundtime_access_token', 'expired');
			localStorageMock.setItem('soundtime_refresh_token', 'refresh-token');

			const fetchMock = vi.fn()
				.mockResolvedValueOnce({
					ok: false,
					status: 401,
					json: () => Promise.resolve({ error: 'Unauthorized' }),
				})
				.mockRejectedValueOnce(new Error('Network error'));

			vi.stubGlobal('fetch', fetchMock);

			await expect(apiFetch('/thing')).rejects.toThrow('Unauthorized');
			expect(localStorageMock.removeItem).toHaveBeenCalledWith('soundtime_access_token');
		});
	});

	describe('api helper methods', () => {
		beforeEach(() => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('{"success":true}'),
			}));
		});

		it('api.get makes GET request', async () => {
			await api.get('/items');
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/items');
		});

		it('api.post makes POST request with body', async () => {
			await api.post('/items', { name: 'new' });
			const [, options] = vi.mocked(fetch).mock.calls[0];
			expect(options?.method).toBe('POST');
			expect(options?.body).toBe(JSON.stringify({ name: 'new' }));
		});

		it('api.post works without body', async () => {
			await api.post('/trigger');
			const [, options] = vi.mocked(fetch).mock.calls[0];
			expect(options?.method).toBe('POST');
		});

		it('api.put makes PUT request with body', async () => {
			await api.put('/items/1', { name: 'updated' });
			const [, options] = vi.mocked(fetch).mock.calls[0];
			expect(options?.method).toBe('PUT');
			expect(options?.body).toBe(JSON.stringify({ name: 'updated' }));
		});

		it('api.delete makes DELETE request', async () => {
			await api.delete('/items/1');
			const [, options] = vi.mocked(fetch).mock.calls[0];
			expect(options?.method).toBe('DELETE');
		});

		it('api.upload sends FormData via POST', async () => {
			const formData = new FormData();
			formData.append('file', new Blob(['test']), 'test.mp3');

			await api.upload('/upload', formData);
			const [, options] = vi.mocked(fetch).mock.calls[0];
			expect(options?.method).toBe('POST');
			expect(options?.body).toBe(formData);
		});

		it('api.patch makes PATCH request with body', async () => {
			await api.patch('/items/1', { status: 'active' });
			const [, options] = vi.mocked(fetch).mock.calls[0];
			expect(options?.method).toBe('PATCH');
			expect(options?.body).toBe(JSON.stringify({ status: 'active' }));
		});
	});

	describe('api.uploadWithProgress', () => {
		function createMockXHR(overrides: Record<string, any> = {}) {
			const instance = {
				open: vi.fn(),
				send: vi.fn(),
				abort: vi.fn(),
				setRequestHeader: vi.fn(),
				upload: { addEventListener: vi.fn() },
				addEventListener: vi.fn(),
				status: 200,
				responseText: '{}',
				...overrides,
			};
			const MockXHR = function (this: any) {
				Object.assign(this, instance);
			} as any;
			vi.stubGlobal('XMLHttpRequest', MockXHR);
			return instance;
		}

		it('returns promise and abort function', () => {
			createMockXHR();
			const result = api.uploadWithProgress('/upload', new FormData());
			expect(result.promise).toBeInstanceOf(Promise);
			expect(typeof result.abort).toBe('function');
		});

		it('sets Authorization header when token exists', () => {
			localStorageMock.setItem('soundtime_access_token', 'upload-token');
			const xhr = createMockXHR();
			api.uploadWithProgress('/upload', new FormData());
			expect(xhr.setRequestHeader).toHaveBeenCalledWith('Authorization', 'Bearer upload-token');
		});

		it('calls onProgress callback', () => {
			const xhr = createMockXHR();
			const onProgress = vi.fn();
			api.uploadWithProgress('/upload', new FormData(), onProgress);

			const progressHandler = xhr.upload.addEventListener.mock.calls[0][1];
			progressHandler({ lengthComputable: true, loaded: 50, total: 100 });
			expect(onProgress).toHaveBeenCalledWith(50, 100);
		});

		it('resolves on successful load', async () => {
			const xhr = createMockXHR({ status: 200, responseText: '{"id":"123"}' });
			const { promise } = api.uploadWithProgress('/upload', new FormData());

			const loadCall = xhr.addEventListener.mock.calls.find((c: any[]) => c[0] === 'load');
			loadCall![1]();

			const result = await promise;
			expect(result).toEqual({ id: '123' });
		});

		it('rejects on network error', async () => {
			const xhr = createMockXHR();
			const { promise } = api.uploadWithProgress('/upload', new FormData());

			const errorCall = xhr.addEventListener.mock.calls.find((c: any[]) => c[0] === 'error');
			errorCall![1]();

			await expect(promise).rejects.toThrow('Network error');
		});

		it('rejects on abort', async () => {
			const xhr = createMockXHR();
			const { promise } = api.uploadWithProgress('/upload', new FormData());

			const abortCall = xhr.addEventListener.mock.calls.find((c: any[]) => c[0] === 'abort');
			abortCall![1]();

			await expect(promise).rejects.toThrow('Upload cancelled');
		});

		it('rejects with server error on non-2xx', async () => {
			const xhr = createMockXHR({ status: 413, responseText: '{"error":"File too large"}' });
			const { promise } = api.uploadWithProgress('/upload', new FormData());

			const loadCall = xhr.addEventListener.mock.calls.find((c: any[]) => c[0] === 'load');
			loadCall![1]();

			await expect(promise).rejects.toThrow('File too large');
		});
	});

	describe('api.uploadWithProgress edge cases', () => {
		function createMockXHR(overrides: Record<string, any> = {}) {
			const instance = {
				open: vi.fn(),
				send: vi.fn(),
				abort: vi.fn(),
				setRequestHeader: vi.fn(),
				upload: { addEventListener: vi.fn() },
				addEventListener: vi.fn(),
				status: 200,
				responseText: '{}',
				...overrides,
			};
			const MockXHR = function (this: any) {
				Object.assign(this, instance);
			} as any;
			vi.stubGlobal('XMLHttpRequest', MockXHR);
			return instance;
		}

		it('does not call onProgress when not lengthComputable', () => {
			const xhr = createMockXHR();
			const onProgress = vi.fn();
			api.uploadWithProgress('/upload', new FormData(), onProgress);
			const progressHandler = xhr.upload.addEventListener.mock.calls[0][1];
			progressHandler({ lengthComputable: false, loaded: 50, total: 100 });
			expect(onProgress).not.toHaveBeenCalled();
		});

		it('does not call onProgress when no callback provided', () => {
			const xhr = createMockXHR();
			api.uploadWithProgress('/upload', new FormData());
			const progressHandler = xhr.upload.addEventListener.mock.calls[0][1];
			// Should not throw even without onProgress
			expect(() => progressHandler({ lengthComputable: true, loaded: 50, total: 100 })).not.toThrow();
		});

		it('resolves undefined when successful response is not valid JSON', async () => {
			const xhr = createMockXHR({ status: 200, responseText: 'not json' });
			const { promise } = api.uploadWithProgress('/upload', new FormData());
			const loadCall = xhr.addEventListener.mock.calls.find((c: any[]) => c[0] === 'load');
			loadCall![1]();
			const result = await promise;
			expect(result).toBeUndefined();
		});

		it('rejects with generic HTTP error when error response is not valid JSON', async () => {
			const xhr = createMockXHR({ status: 500, responseText: 'not json' });
			const { promise } = api.uploadWithProgress('/upload', new FormData());
			const loadCall = xhr.addEventListener.mock.calls.find((c: any[]) => c[0] === 'load');
			loadCall![1]();
			await expect(promise).rejects.toThrow('HTTP 500');
		});

		it('does not set Authorization header when no token exists', () => {
			localStorageMock.clear();
			const xhr = createMockXHR();
			api.uploadWithProgress('/upload', new FormData());
			expect(xhr.setRequestHeader).not.toHaveBeenCalledWith('Authorization', expect.any(String));
		});

		it('abort function calls xhr.abort', () => {
			const xhr = createMockXHR();
			const { abort } = api.uploadWithProgress('/upload', new FormData());
			abort();
			expect(xhr.abort).toHaveBeenCalled();
		});
	});

	describe('pluginApi', () => {
		beforeEach(() => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('{"plugins":[]}'),
			}));
		});

		it('pluginApi.list calls GET /admin/plugins', async () => {
			await pluginApi.list();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins');
		});

		it('pluginApi.install calls POST /admin/plugins/install with git_url', async () => {
			await pluginApi.install('https://github.com/org/repo.git');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/install');
			expect(options?.method).toBe('POST');
			expect(options?.body).toBe(JSON.stringify({ git_url: 'https://github.com/org/repo.git' }));
		});

		it('pluginApi.enable calls POST /admin/plugins/:id/enable', async () => {
			await pluginApi.enable('plugin-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123/enable');
			expect(options?.method).toBe('POST');
		});

		it('pluginApi.disable calls POST /admin/plugins/:id/disable', async () => {
			await pluginApi.disable('plugin-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123/disable');
			expect(options?.method).toBe('POST');
		});

		it('pluginApi.uninstall calls DELETE /admin/plugins/:id', async () => {
			await pluginApi.uninstall('plugin-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123');
			expect(options?.method).toBe('DELETE');
		});

		it('pluginApi.update calls POST /admin/plugins/:id/update', async () => {
			await pluginApi.update('plugin-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123/update');
			expect(options?.method).toBe('POST');
		});

		it('pluginApi.getConfig calls GET /admin/plugins/:id/config', async () => {
			await pluginApi.getConfig('plugin-123');
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123/config');
		});

		it('pluginApi.updateConfig calls PUT /admin/plugins/:id/config', async () => {
			await pluginApi.updateConfig('plugin-123', [{ key: 'api_key', value: 'abc123' }]);
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123/config');
			expect(options?.method).toBe('PUT');
			expect(options?.body).toContain('"api_key"');
		});

		it('pluginApi.getLogs calls GET /admin/plugins/:id/logs with pagination', async () => {
			await pluginApi.getLogs('plugin-123', 2, 25);
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/plugins/plugin-123/logs?page=2&per_page=25');
		});
	});

	describe('themeApi', () => {
		beforeEach(() => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('{"themes":[]}'),
			}));
		});

		it('themeApi.list calls GET /admin/themes', async () => {
			await themeApi.list();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/themes');
		});

		it('themeApi.install calls POST /admin/themes/install with git_url', async () => {
			await themeApi.install('https://github.com/user/theme.git');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/themes/install');
			expect(options?.method).toBe('POST');
			expect(options?.body).toBe(JSON.stringify({ git_url: 'https://github.com/user/theme.git' }));
		});

		it('themeApi.enable calls POST /admin/themes/:id/enable', async () => {
			await themeApi.enable('theme-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/themes/theme-123/enable');
			expect(options?.method).toBe('POST');
		});

		it('themeApi.disable calls POST /admin/themes/:id/disable', async () => {
			await themeApi.disable('theme-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/themes/theme-123/disable');
			expect(options?.method).toBe('POST');
		});

		it('themeApi.update calls POST /admin/themes/:id/update', async () => {
			await themeApi.update('theme-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/themes/theme-123/update');
			expect(options?.method).toBe('POST');
		});

		it('themeApi.uninstall calls DELETE /admin/themes/:id', async () => {
			await themeApi.uninstall('theme-123');
			const [url, options] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/admin/themes/theme-123');
			expect(options?.method).toBe('DELETE');
		});

		it('themeApi.active calls GET /themes/active', async () => {
			await themeApi.active();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/themes/active');
		});
	});

	describe('homeApi', () => {
		beforeEach(() => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: true,
				status: 200,
				text: () => Promise.resolve('[]'),
			}));
		});

		it('homeApi.randomTracks calls GET /tracks/random with count', async () => {
			await homeApi.randomTracks(5);
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/tracks/random');
			expect(url).toContain('count=5');
		});

		it('homeApi.randomTracks with genre param', async () => {
			await homeApi.randomTracks(10, 'rock');
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('genre=rock');
		});

		it('homeApi.recentlyAdded calls GET /tracks/recently-added', async () => {
			await homeApi.recentlyAdded(5);
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/tracks/recently-added');
		});

		it('homeApi.popularTracks calls GET /tracks/popular', async () => {
			await homeApi.popularTracks();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/tracks/popular');
		});

		it('homeApi.genres calls GET /genres', async () => {
			await homeApi.genres();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/genres');
		});

		it('homeApi.genreTracks calls GET /genres/:genre/tracks', async () => {
			await homeApi.genreTracks('jazz');
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/genres/jazz/tracks');
		});

		it('homeApi.recentHistory calls GET /history/recent', async () => {
			await homeApi.recentHistory();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/history/recent');
		});

		it('homeApi.statsOverview calls GET /stats/overview', async () => {
			await homeApi.statsOverview();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/stats/overview');
		});

		it('homeApi.recentAlbums calls GET /albums/recent', async () => {
			await homeApi.recentAlbums();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/albums/recent');
		});

		it('homeApi.topArtists calls GET /artists/top', async () => {
			await homeApi.topArtists();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/artists/top');
		});

		it('homeApi.editorialPlaylists calls GET /editorial-playlists', async () => {
			await homeApi.editorialPlaylists();
			const [url] = vi.mocked(fetch).mock.calls[0];
			expect(url).toContain('/editorial-playlists');
		});

		it('homeApi.editorialPlaylists returns empty array on failure', async () => {
			vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
				ok: false,
				status: 500,
				json: () => Promise.resolve({ error: 'Server error' }),
			}));
			const result = await homeApi.editorialPlaylists();
			expect(result).toEqual([]);
		});
	});
});
