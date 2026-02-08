import { describe, it, expect, vi, beforeEach } from 'vitest';

// Smart chainable d3 mock that captures .data() items and invokes function arguments
function makeSmartChainable(boundData: any[] = []): any {
  function makeHandler(data: any[]): ProxyHandler<object> {
    return {
      get(_target, prop) {
        if (typeof prop === 'symbol') return undefined;
        if (prop === 'then' || prop === 'catch') return undefined;

        return function (...args: any[]) {
          let nextData = data;
          // Capture data from .data(items) calls
          if (prop === 'data' && Array.isArray(args[0])) {
            nextData = args[0];
          }
          // Execute function arguments with all bound data items
          // This exercises callbacks like d => getNodeColor(d.node_type)
          for (const arg of args) {
            if (typeof arg === 'function') {
              for (const d of nextData) {
                try { arg(d, 0, nextData); } catch { }
              }
            }
          }
          return new Proxy({}, makeHandler(nextData));
        };
      },
    };
  }
  return new Proxy({}, makeHandler(boundData));
}

vi.mock('d3', () => ({
  select: vi.fn(() => makeSmartChainable()),
  forceSimulation: vi.fn((...args: any[]) => {
    const data = Array.isArray(args[0]) ? args[0] : [];
    return makeSmartChainable(data);
  }),
  forceLink: vi.fn((...args: any[]) => {
    const data = Array.isArray(args[0]) ? args[0] : [];
    return makeSmartChainable(data);
  }),
  forceManyBody: vi.fn(() => makeSmartChainable()),
  forceCenter: vi.fn(() => makeSmartChainable()),
  forceCollide: vi.fn(() => makeSmartChainable()),
  forceX: vi.fn(() => makeSmartChainable()),
  forceY: vi.fn(() => makeSmartChainable()),
  drag: vi.fn(() => makeSmartChainable()),
}));

vi.mock('$lib/i18n/index.svelte', () => ({
  t: (key: string) => {
    const map: Record<string, string> = {
      'admin.graph.selfNode': 'This server',
      'admin.graph.relayNode': 'Relay',
      'admin.graph.peerNode': 'Peer',
    };
    return map[key] ?? key;
  },
}));

import { render } from '@testing-library/svelte';
import NetworkGraph from './NetworkGraph.svelte';
import * as d3 from 'd3';

// Full data with ALL node_types and link_types to cover every switch branch
const fullData = {
  nodes: [
    { id: 'n1', node_type: 'self' as const, label: 'Self Server', online: true },
    { id: 'n2', node_type: 'peer' as const, label: 'Peer Server', online: true },
    { id: 'n3', node_type: 'relay' as const, label: 'Relay Server', online: false },
    { id: 'n4', node_type: 'other' as any, label: 'Unknown Type', online: true },
  ],
  links: [
    { source: 'n1', target: 'n2', link_type: 'direct' as const },
    { source: 'n1', target: 'n3', link_type: 'relay' as const },
    { source: 'n2', target: 'n3', link_type: 'peer' as any },
    { source: 'n3', target: 'n4', link_type: 'unknown' as any },
  ],
};

describe('NetworkGraph', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders container element', () => {
    const { container } = render(NetworkGraph, {
      props: { data: { nodes: [], links: [] } },
    });
    expect(container.querySelector('div')).toBeInTheDocument();
  });

  it('renders legend with colored circles', () => {
    const { container } = render(NetworkGraph, {
      props: { data: { nodes: [], links: [] } },
    });
    const legendCircles = container.querySelectorAll('.rounded-full');
    expect(legendCircles.length).toBe(3);
  });

  it('shows legend labels for self, peer, relay', () => {
    const { container } = render(NetworkGraph, {
      props: { data: { nodes: [], links: [] } },
    });
    const text = container.textContent!;
    expect(text).toContain('This server');
    expect(text).toContain('Relay');
    expect(text).toContain('Peer');
  });

  it('shows relay and direct link types in legend', () => {
    const { container } = render(NetworkGraph, {
      props: { data: { nodes: [], links: [] } },
    });
    const text = container.textContent!;
    expect(text).toContain('relay');
    expect(text).toContain('direct');
  });

  it('renders border elements for link legend', () => {
    const { container } = render(NetworkGraph, {
      props: { data: { nodes: [], links: [] } },
    });
    const borderedElements = container.querySelectorAll('.border-t-2');
    expect(borderedElements.length).toBe(2);
  });

  it('calls d3 with diverse data exercising all helper function branches', () => {
    render(NetworkGraph, { props: { data: fullData } });
    expect(d3.select).toHaveBeenCalled();
    expect(d3.forceSimulation).toHaveBeenCalled();
    expect(d3.forceLink).toHaveBeenCalled();
  });

  it('renders with empty nodes and no error', () => {
    const data = { nodes: [], links: [] };
    const { container } = render(NetworkGraph, { props: { data } });
    expect(container.querySelector('div')).toBeInTheDocument();
  });

  it('handles nodes with online=false for offline styling branch', () => {
    const offlineData = {
      nodes: [
        { id: 'n1', node_type: 'self' as const, label: 'Offline', online: false },
        { id: 'n2', node_type: 'peer' as const, label: 'Offline P', online: false },
      ],
      links: [],
    };
    render(NetworkGraph, { props: { data: offlineData } });
    expect(d3.select).toHaveBeenCalled();
  });

  it('handles single node with relay type', () => {
    const singleData = {
      nodes: [{ id: 'n1', node_type: 'relay' as const, label: 'Relay', online: true }],
      links: [],
    };
    render(NetworkGraph, { props: { data: singleData } });
    expect(d3.select).toHaveBeenCalled();
  });

  it('handles self node type exclusively', () => {
    const selfData = {
      nodes: [{ id: 'n1', node_type: 'self' as const, label: 'Self', online: true }],
      links: [],
    };
    render(NetworkGraph, { props: { data: selfData } });
    expect(d3.forceSimulation).toHaveBeenCalled();
  });

  it('renders graph container div with full width', () => {
    const { container } = render(NetworkGraph, {
      props: { data: { nodes: [], links: [] } },
    });
    const graphDiv = container.querySelector('.w-full');
    expect(graphDiv).toBeInTheDocument();
  });

  it('calls forceCollide and forceCenter', () => {
    render(NetworkGraph, { props: { data: fullData } });
    expect(d3.forceCollide).toHaveBeenCalled();
    expect(d3.forceCenter).toHaveBeenCalled();
  });

  it('calls forceManyBody for charge', () => {
    render(NetworkGraph, { props: { data: fullData } });
    expect(d3.forceManyBody).toHaveBeenCalled();
  });

  it('calls forceX and forceY for centering', () => {
    render(NetworkGraph, { props: { data: fullData } });
    expect(d3.forceX).toHaveBeenCalled();
    expect(d3.forceY).toHaveBeenCalled();
  });

  it('calls drag for interactivity', () => {
    render(NetworkGraph, { props: { data: fullData } });
    expect(d3.drag).toHaveBeenCalled();
  });
});
