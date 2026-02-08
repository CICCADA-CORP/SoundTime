import { describe, it, expect, vi, beforeEach } from 'vitest';

const { mockApi, mockAuth } = vi.hoisted(() => {
  return {
    mockApi: {
      get: vi.fn(),
      post: vi.fn().mockResolvedValue(undefined),
      delete: vi.fn().mockResolvedValue(undefined),
    },
    mockAuth: {
      isAuthenticated: false,
      user: null as any,
    },
  };
});

vi.mock('$lib/api', () => ({
  api: mockApi,
  API_BASE: '/api',
}));

vi.mock('$lib/stores/auth.svelte', () => ({
  getAuthStore: () => mockAuth,
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => key,
}));

import { render, screen, fireEvent } from '@testing-library/svelte';
import FavoriteButton from './FavoriteButton.svelte';

describe('FavoriteButton', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockAuth.isAuthenticated = false;
    mockAuth.user = null;
  });

  it('renders nothing when user is not authenticated', () => {
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1' } });
    expect(container.querySelector('button')).toBeNull();
  });

  it('renders a button when user is authenticated', () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1' } });
    expect(container.querySelector('button')).toBeInTheDocument();
  });

  it('renders unfilled heart when not liked', () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1', liked: false } });
    const svg = container.querySelector('svg');
    expect(svg).toBeInTheDocument();
    expect(svg?.getAttribute('fill')).toBe('none');
  });

  it('renders filled heart when liked', () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1', liked: true } });
    const svg = container.querySelector('svg');
    expect(svg).toBeInTheDocument();
    expect(svg?.getAttribute('fill')).toContain('hsl');
  });

  it('calls api.post when clicking unfilled heart', async () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1', liked: false } });
    const button = container.querySelector('button')!;
    await fireEvent.click(button);
    expect(mockApi.post).toHaveBeenCalledWith('/favorites/track-1');
  });

  it('calls api.delete when clicking filled heart', async () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1', liked: true } });
    const button = container.querySelector('button')!;
    await fireEvent.click(button);
    expect(mockApi.delete).toHaveBeenCalledWith('/favorites/track-1');
  });

  it('applies custom size', () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1', size: 24 } });
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('width')).toBe('24');
    expect(svg?.getAttribute('height')).toBe('24');
  });

  it('has title attribute for accessibility', () => {
    mockAuth.isAuthenticated = true; mockAuth.user = { id: '1', username: 'alice' };
    const { container } = render(FavoriteButton, { props: { trackId: 'track-1' } });
    const button = container.querySelector('button');
    expect(button?.getAttribute('title')).toBeTruthy();
  });
});
