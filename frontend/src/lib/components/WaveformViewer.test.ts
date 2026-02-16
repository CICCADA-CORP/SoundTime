import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import WaveformViewer from './WaveformViewer.svelte';

describe('WaveformViewer', () => {
  it('renders with default props', () => {
    const { container } = render(WaveformViewer);
    const div = container.querySelector('div');
    expect(div).toBeInTheDocument();
  });

  it('renders SVG element', () => {
    const { container } = render(WaveformViewer, { props: { data: [0.5, 0.8, 0.3], progress: 0 } });
    const svg = container.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('renders bars for each data point', () => {
    const data = [0.5, 0.8, 0.3, 0.9, 0.2];
    const { container } = render(WaveformViewer, { props: { data, progress: 0 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(data.length);
  });

  it('applies custom height', () => {
    const { container } = render(WaveformViewer, { props: { data: [0.5], progress: 0, height: 100 } });
    const div = container.querySelector('div');
    expect(div?.getAttribute('style')).toContain('height: 100px');
  });

  it('renders no bars when data is empty', () => {
    const { container } = render(WaveformViewer, { props: { data: [], progress: 0 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(0);
  });

  it('applies past color to bars before progress point', () => {
    const data = [0.5, 0.8, 0.3, 0.9, 0.2];
    const { container } = render(WaveformViewer, { props: { data, progress: 0.6 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(data.length);
    // First bars should have the "past" color (green), later ones the "future" color (gray)
    const firstFill = rects[0]?.getAttribute('fill');
    const lastFill = rects[rects.length - 1]?.getAttribute('fill');
    expect(firstFill).toContain('142'); // hsl(142, 71%, 45%)
    expect(lastFill).toContain('0, 0%'); // hsl(0, 0%, 25%)
  });

  it('all bars are past when progress is 1', () => {
    const data = [0.5, 0.8, 0.3];
    const { container } = render(WaveformViewer, { props: { data, progress: 1 } });
    const rects = container.querySelectorAll('rect');
    for (const rect of rects) {
      expect(rect.getAttribute('fill')).toContain('142');
    }
  });

  it('all bars are future when progress is 0', () => {
    const data = [0.5, 0.8, 0.3];
    const { container } = render(WaveformViewer, { props: { data, progress: 0 } });
    const rects = container.querySelectorAll('rect');
    for (const rect of rects) {
      expect(rect.getAttribute('fill')).toContain('0, 0%');
    }
  });

  it('handles very small peak values with minimum height', () => {
    const data = [0.001, 0.001];
    const { container } = render(WaveformViewer, { props: { data, progress: 0.5 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(2);
  });

  it('renders with single data point', () => {
    const { container } = render(WaveformViewer, { props: { data: [1.0], progress: 0.5 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(1);
  });

  it('handles progress at exactly 0.5', () => {
    const data = [0.5, 0.5];
    const { container } = render(WaveformViewer, { props: { data, progress: 0.5 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(2);
  });

  it('handles large data arrays', () => {
    const data = Array.from({ length: 100 }, () => Math.random());
    const { container } = render(WaveformViewer, { props: { data, progress: 0.5 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(100);
  });

  it('uses default height of 60 when not specified', () => {
    const { container } = render(WaveformViewer, { props: { data: [0.5], progress: 0 } });
    const div = container.querySelector('div');
    expect(div?.getAttribute('style')).toContain('height: 60px');
  });

  it('handles progress at 0.5 correctly - first half past, second half future', () => {
    const data = [0.5, 0.5, 0.5, 0.5];
    const { container } = render(WaveformViewer, { props: { data, progress: 0.5 } });
    const rects = container.querySelectorAll('rect');
    // First two bars should be "past" (x < 50%), last two "future"
    expect(rects[0]?.getAttribute('fill')).toContain('142');
    expect(rects[rects.length - 1]?.getAttribute('fill')).toContain('0, 0%');
  });

  it('renders correctly with data length of 2', () => {
    const data = [0.8, 0.3];
    const { container } = render(WaveformViewer, { props: { data, progress: 0.25 } });
    const rects = container.querySelectorAll('rect');
    expect(rects.length).toBe(2);
    // First bar at x=0% (0/2*100), progress=25%, so 0 < 25 → past
    expect(rects[0]?.getAttribute('fill')).toContain('142');
    // Second bar at x=50% (1/2*100), 50 < 25 is false → future
    expect(rects[1]?.getAttribute('fill')).toContain('0, 0%');
  });
});
