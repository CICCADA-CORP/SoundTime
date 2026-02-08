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
    apiFetch: vi.fn(),
    setTokens: vi.fn(),
    clearTokens: vi.fn(),
  };
});

import { getAuthStore } from './auth.svelte';
import { api, setTokens, clearTokens, apiFetch } from '$lib/api';

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

describe('Auth Store', () => {
  let auth: ReturnType<typeof getAuthStore>;

  beforeEach(() => {
    vi.clearAllMocks();
    localStorageMock.clear();
    auth = getAuthStore();
    // Reset module-level $state between tests
    auth.logout();
  });

  describe('initial state', () => {
    it('user is null by default', () => {
      expect(auth.user).toBeNull();
    });

    it('isAuthenticated is false when no user', () => {
      expect(auth.isAuthenticated).toBe(false);
    });

    it('isAdmin is false when no user', () => {
      expect(auth.isAdmin).toBe(false);
    });
  });

  describe('login', () => {
    it('calls api.post with credentials and sets user', async () => {
      const mockUser = { id: '1', username: 'alice', role: 'user', email: 'a@b.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      const mockResponse = {
        user: mockUser,
        tokens: { access_token: 'at', refresh_token: 'rt' },
      };
      vi.mocked(api.post).mockResolvedValueOnce(mockResponse);

      await auth.login('alice', 'pass123');

      expect(api.post).toHaveBeenCalledWith('/auth/login', { username: 'alice', password: 'pass123' });
      expect(setTokens).toHaveBeenCalledWith('at', 'rt');
      expect(auth.user).toEqual(mockUser);
      expect(auth.isAuthenticated).toBe(true);
    });

    it('throws when api.post fails', async () => {
      vi.mocked(api.post).mockRejectedValueOnce(new Error('Invalid credentials'));

      await expect(auth.login('alice', 'bad')).rejects.toThrow('Invalid credentials');
      expect(auth.user).toBeNull();
    });
  });

  describe('register', () => {
    it('calls api.post with registration data and sets user', async () => {
      const mockUser = { id: '2', username: 'bob', role: 'user', email: 'b@c.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      const mockResponse = {
        user: mockUser,
        tokens: { access_token: 'at2', refresh_token: 'rt2' },
      };
      vi.mocked(api.post).mockResolvedValueOnce(mockResponse);

      await auth.register('b@c.com', 'bob', 'pass456');

      expect(api.post).toHaveBeenCalledWith('/auth/register', { email: 'b@c.com', username: 'bob', password: 'pass456' });
      expect(setTokens).toHaveBeenCalledWith('at2', 'rt2');
      expect(auth.user).toEqual(mockUser);
    });
  });

  describe('logout', () => {
    it('clears tokens and resets user', async () => {
      const mockUser = { id: '1', username: 'alice', role: 'user', email: 'a@b.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      vi.mocked(api.post).mockResolvedValueOnce({
        user: mockUser,
        tokens: { access_token: 'at', refresh_token: 'rt' },
      });
      await auth.login('alice', 'pass');

      auth.logout();

      expect(clearTokens).toHaveBeenCalled();
      expect(auth.user).toBeNull();
      expect(auth.isAuthenticated).toBe(false);
    });
  });

  describe('deleteAccount', () => {
    it('calls apiFetch DELETE and clears state', async () => {
      // First login
      const mockUser = { id: '1', username: 'alice', role: 'user', email: 'a@b.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      vi.mocked(api.post).mockResolvedValueOnce({
        user: mockUser,
        tokens: { access_token: 'at', refresh_token: 'rt' },
      });
      await auth.login('alice', 'pass');
      vi.mocked(apiFetch).mockResolvedValueOnce(undefined);

      await auth.deleteAccount('pass');

      expect(apiFetch).toHaveBeenCalledWith('/auth/account', {
        method: 'DELETE',
        body: JSON.stringify({ password: 'pass' }),
      });
      expect(clearTokens).toHaveBeenCalled();
      expect(auth.user).toBeNull();
    });
  });

  describe('updateEmail', () => {
    it('calls api.put and updates user', async () => {
      const updatedUser = { id: '1', username: 'alice', role: 'user', email: 'new@email.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      vi.mocked(api.put).mockResolvedValueOnce(updatedUser);

      await auth.updateEmail('new@email.com', 'pass');

      expect(api.put).toHaveBeenCalledWith('/auth/email', { new_email: 'new@email.com', password: 'pass' });
      expect(auth.user).toEqual(updatedUser);
    });
  });

  describe('updatePassword', () => {
    it('calls apiFetch PUT', async () => {
      vi.mocked(apiFetch).mockResolvedValueOnce(undefined);

      await auth.updatePassword('old', 'new');

      expect(apiFetch).toHaveBeenCalledWith('/auth/password', {
        method: 'PUT',
        body: JSON.stringify({ current_password: 'old', new_password: 'new' }),
      });
    });
  });

  describe('fetchMe', () => {
    it('sets user when api call succeeds', async () => {
      const mockUser = { id: '1', username: 'alice', role: 'admin', email: 'a@b.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      vi.mocked(api.get).mockResolvedValueOnce(mockUser);

      await auth.fetchMe();

      expect(api.get).toHaveBeenCalledWith('/auth/me');
      expect(auth.user).toEqual(mockUser);
      expect(auth.isAdmin).toBe(true);
    });

    it('sets user to null when api call fails', async () => {
      vi.mocked(api.get).mockRejectedValueOnce(new Error('not authed'));

      await auth.fetchMe();

      expect(auth.user).toBeNull();
    });
  });

  describe('init', () => {
    it('calls fetchMe when token exists in localStorage', async () => {
      localStorageMock.setItem('soundtime_access_token', 'token');
      const mockUser = { id: '1', username: 'alice', role: 'user', email: 'a@b.com', display_name: null, avatar_url: null, instance_id: 'i1', created_at: '2025-01-01' };
      vi.mocked(api.get).mockResolvedValueOnce(mockUser);

      await auth.init();

      expect(api.get).toHaveBeenCalledWith('/auth/me');
      expect(auth.loading).toBe(false);
    });

    it('does not call fetchMe when no token in localStorage', async () => {
      await auth.init();

      expect(api.get).not.toHaveBeenCalled();
      expect(auth.loading).toBe(false);
    });
  });
});
