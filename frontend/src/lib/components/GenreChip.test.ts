import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import GenreChip from './GenreChip.svelte';

describe('GenreChip', () => {
  it('renders genre text', () => {
    render(GenreChip, { props: { genre: 'Rock' } });
    expect(screen.getByText('Rock')).toBeInTheDocument();
  });

  it('renders as a button element', () => {
    render(GenreChip, { props: { genre: 'Jazz' } });
    expect(screen.getByRole('button', { name: 'Jazz' })).toBeInTheDocument();
  });

  it('renders with inactive styling by default (active=false)', () => {
    const { container } = render(GenreChip, { props: { genre: 'Pop' } });
    const button = container.querySelector('button')!;
    expect(button.className).toContain('bg-[hsl(var(--secondary))]');
    expect(button.className).not.toContain('bg-[hsl(var(--primary))]');
  });

  it('renders with active styling when active=true', () => {
    const { container } = render(GenreChip, { props: { genre: 'Pop', active: true } });
    const button = container.querySelector('button')!;
    expect(button.className).toContain('bg-[hsl(var(--primary))]');
    expect(button.className).toContain('text-white');
    expect(button.className).toContain('shadow-md');
  });

  it('renders with inactive styling when active=false explicitly', () => {
    const { container } = render(GenreChip, { props: { genre: 'Electronic', active: false } });
    const button = container.querySelector('button')!;
    expect(button.className).toContain('bg-[hsl(var(--secondary))]');
    expect(button.className).toContain('text-[hsl(var(--foreground))]');
  });

  it('calls onclick when clicked', async () => {
    const handleClick = vi.fn();
    render(GenreChip, { props: { genre: 'Blues', onclick: handleClick } });
    const button = screen.getByRole('button', { name: 'Blues' });
    await fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });

  it('does not throw when clicked without onclick handler', async () => {
    render(GenreChip, { props: { genre: 'Metal' } });
    const button = screen.getByRole('button', { name: 'Metal' });
    await fireEvent.click(button);
    // Should not throw
    expect(button).toBeInTheDocument();
  });
});
