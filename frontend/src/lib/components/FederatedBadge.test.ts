import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import FederatedBadge from './FederatedBadge.svelte';

describe('FederatedBadge', () => {
  it('renders with default text when no instance provided', () => {
    render(FederatedBadge);
    expect(screen.getByText('federated')).toBeInTheDocument();
  });

  it('renders with instance name when provided', () => {
    render(FederatedBadge, { props: { instance: 'music.example.com' } });
    expect(screen.getByText('music.example.com')).toBeInTheDocument();
  });

  it('renders as a span element', () => {
    const { container } = render(FederatedBadge);
    const span = container.querySelector('span');
    expect(span).toBeInTheDocument();
  });

  it('contains an SVG icon', () => {
    const { container } = render(FederatedBadge);
    const svg = container.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('renders "federated" when instance is empty string', () => {
    render(FederatedBadge, { props: { instance: '' } });
    expect(screen.getByText('federated')).toBeInTheDocument();
  });
});
