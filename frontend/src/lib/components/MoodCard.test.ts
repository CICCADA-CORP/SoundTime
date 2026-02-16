import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import MoodCard from './MoodCard.svelte';

describe('MoodCard', () => {
  const defaultProps = {
    mood: 'Chill',
    gradient: 'bg-gradient-to-br from-blue-500 to-purple-600',
    emoji: '🎧',
  };

  it('renders mood text', () => {
    render(MoodCard, { props: defaultProps });
    expect(screen.getByText('Chill')).toBeInTheDocument();
  });

  it('renders emoji', () => {
    render(MoodCard, { props: defaultProps });
    expect(screen.getByText('🎧')).toBeInTheDocument();
  });

  it('renders a button element', () => {
    render(MoodCard, { props: defaultProps });
    expect(screen.getByRole('button')).toBeInTheDocument();
  });

  it('applies gradient class', () => {
    const { container } = render(MoodCard, { props: defaultProps });
    const gradientDiv = container.querySelector('.bg-gradient-to-br');
    expect(gradientDiv).toBeInTheDocument();
  });

  it('calls onclick when clicked', async () => {
    const handleClick = vi.fn();
    render(MoodCard, { props: { ...defaultProps, onclick: handleClick } });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(handleClick).toHaveBeenCalledTimes(1);
  });

  it('does not throw when clicked without onclick handler', async () => {
    render(MoodCard, { props: defaultProps });
    const button = screen.getByRole('button');
    await fireEvent.click(button);
    expect(button).toBeInTheDocument();
  });

  it('applies different gradient classes correctly', () => {
    const { container } = render(MoodCard, {
      props: { mood: 'Energetic', gradient: 'bg-gradient-to-r from-red-500 to-yellow-500', emoji: '🔥' }
    });
    const gradientDiv = container.querySelector('.bg-gradient-to-r');
    expect(gradientDiv).toBeInTheDocument();
  });

  it('renders with empty gradient string', () => {
    const { container } = render(MoodCard, {
      props: { mood: 'None', gradient: '', emoji: '😶' }
    });
    expect(screen.getByText('None')).toBeInTheDocument();
  });
});
