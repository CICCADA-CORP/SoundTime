import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => key,
}));

import SectionCarouselTest from './SectionCarouselTest.svelte';

describe('SectionCarousel', () => {
  it('renders the title', () => {
    render(SectionCarouselTest, { props: { title: 'My Section' } });
    expect(screen.getByText('My Section')).toBeInTheDocument();
  });

  it('renders children content', () => {
    render(SectionCarouselTest, { props: { title: 'Test' } });
    expect(screen.getByTestId('carousel-child')).toBeInTheDocument();
    expect(screen.getByText('Child content')).toBeInTheDocument();
  });

  it('renders scroll left button with correct aria-label', () => {
    render(SectionCarouselTest, { props: { title: 'Test' } });
    const leftButton = screen.getByLabelText('a11y.scrollLeft');
    expect(leftButton).toBeInTheDocument();
  });

  it('renders scroll right button with correct aria-label', () => {
    render(SectionCarouselTest, { props: { title: 'Test' } });
    const rightButton = screen.getByLabelText('a11y.scrollRight');
    expect(rightButton).toBeInTheDocument();
  });

  it('left button is initially disabled (canScrollLeft starts false)', () => {
    render(SectionCarouselTest, { props: { title: 'Test' } });
    const leftButton = screen.getByLabelText('a11y.scrollLeft');
    expect(leftButton).toBeDisabled();
  });

  it('right button is initially enabled (canScrollRight starts true)', () => {
    render(SectionCarouselTest, { props: { title: 'Test' } });
    const rightButton = screen.getByLabelText('a11y.scrollRight');
    expect(rightButton).not.toBeDisabled();
  });

  it('does NOT render "View All" link when href is not provided', () => {
    render(SectionCarouselTest, { props: { title: 'Test' } });
    expect(screen.queryByText('explore.viewAll')).not.toBeInTheDocument();
  });

  it('renders "View All" link when href is provided', () => {
    render(SectionCarouselTest, { props: { title: 'Test', href: '/tracks' } });
    const link = screen.getByText('explore.viewAll');
    expect(link).toBeInTheDocument();
    expect(link.closest('a')).toHaveAttribute('href', '/tracks');
  });

  it('renders section element', () => {
    const { container } = render(SectionCarouselTest, { props: { title: 'Test' } });
    expect(container.querySelector('section')).toBeInTheDocument();
  });

  it('renders title as h2 heading', () => {
    render(SectionCarouselTest, { props: { title: 'My Section' } });
    const heading = screen.getByRole('heading', { level: 2 });
    expect(heading).toHaveTextContent('My Section');
  });
});
