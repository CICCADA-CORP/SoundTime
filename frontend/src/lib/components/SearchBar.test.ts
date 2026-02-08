import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('$app/navigation', () => ({
  goto: vi.fn(),
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => {
    const translations: Record<string, string> = {
      'search.placeholder': 'Search music...',
    };
    return translations[key] ?? key;
  },
}));

import { render, screen, fireEvent } from '@testing-library/svelte';
import SearchBar from './SearchBar.svelte';
import { goto } from '$app/navigation';

describe('SearchBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders input with placeholder', () => {
    render(SearchBar);
    const input = screen.getByPlaceholderText('Search music...');
    expect(input).toBeInTheDocument();
  });

  it('renders search icon SVG', () => {
    const { container } = render(SearchBar);
    const svg = container.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('navigates to search page on Enter', async () => {
    render(SearchBar);
    const input = screen.getByPlaceholderText('Search music...');

    await fireEvent.input(input, { target: { value: 'test query' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(goto).toHaveBeenCalledWith('/search?q=test%20query');
  });

  it('does not navigate on Enter with empty query', async () => {
    render(SearchBar);
    const input = screen.getByPlaceholderText('Search music...');

    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(goto).not.toHaveBeenCalled();
  });

  it('does not navigate on non-Enter keys', async () => {
    render(SearchBar);
    const input = screen.getByPlaceholderText('Search music...');

    await fireEvent.input(input, { target: { value: 'test' } });
    await fireEvent.keyDown(input, { key: 'a' });

    expect(goto).not.toHaveBeenCalled();
  });
});
