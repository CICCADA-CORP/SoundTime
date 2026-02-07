<script lang="ts">
  import { onMount } from "svelte";
  import * as d3 from "d3";
  import { t } from "$lib/i18n/index.svelte";

  interface GraphNode extends d3.SimulationNodeDatum {
    id: string;
    node_type: "self" | "peer" | "relay";
    label: string;
    online: boolean;
  }

  interface GraphLink extends d3.SimulationLinkDatum<GraphNode> {
    source: string | GraphNode;
    target: string | GraphNode;
    link_type: "relay" | "direct" | "peer";
  }

  interface GraphData {
    nodes: GraphNode[];
    links: GraphLink[];
  }

  let { data = $bindable<GraphData>({ nodes: [], links: [] }) }: { data: GraphData } = $props();

  let container: HTMLDivElement;

  function getNodeColor(type: string): string {
    switch (type) {
      case "self": return "#22c55e";   // green-500
      case "relay": return "#3b82f6";  // blue-500
      case "peer": return "#a855f7";   // purple-500
      default: return "#6b7280";       // gray-500
    }
  }

  function getNodeRadius(type: string): number {
    switch (type) {
      case "self": return 24;
      case "relay": return 18;
      case "peer": return 14;
      default: return 12;
    }
  }

  function getLinkColor(type: string): string {
    switch (type) {
      case "relay": return "#3b82f6";
      case "direct": return "#22c55e";
      case "peer": return "#a855f7";
      default: return "#4b5563";
    }
  }

  function getLinkDash(type: string): string {
    switch (type) {
      case "relay": return "6,3";
      case "direct": return "none";
      default: return "4,4";
    }
  }

  function getNodeIcon(type: string): string {
    switch (type) {
      case "self": return "⬢";
      case "relay": return "◆";
      case "peer": return "●";
      default: return "○";
    }
  }

  function renderGraph() {
    if (!container || !data.nodes.length) return;

    // Clear previous
    d3.select(container).selectAll("*").remove();

    const width = container.clientWidth;
    const height = Math.max(400, container.clientHeight);

    const svg = d3.select(container)
      .append("svg")
      .attr("width", width)
      .attr("height", height)
      .attr("viewBox", [0, 0, width, height]);

    // Defs for glow effect
    const defs = svg.append("defs");
    const filter = defs.append("filter").attr("id", "glow");
    filter.append("feGaussianBlur").attr("stdDeviation", "3.5").attr("result", "coloredBlur");
    const feMerge = filter.append("feMerge");
    feMerge.append("feMergeNode").attr("in", "coloredBlur");
    feMerge.append("feMergeNode").attr("in", "SourceGraphic");

    // Arrow markers for directed links
    defs.selectAll("marker")
      .data(["relay", "direct", "peer"])
      .join("marker")
      .attr("id", d => `arrow-${d}`)
      .attr("viewBox", "0 -5 10 10")
      .attr("refX", 20)
      .attr("refY", 0)
      .attr("markerWidth", 6)
      .attr("markerHeight", 6)
      .attr("orient", "auto")
      .append("path")
      .attr("d", "M0,-5L10,0L0,5")
      .attr("fill", d => getLinkColor(d));

    // Deep-clone nodes/links so D3 can mutate them
    const nodes: GraphNode[] = data.nodes.map(d => ({ ...d }));
    const links: GraphLink[] = data.links.map(d => ({ ...d }));

    const simulation = d3.forceSimulation<GraphNode>(nodes)
      .force("link", d3.forceLink<GraphNode, GraphLink>(links)
        .id(d => d.id)
        .distance(120))
      .force("charge", d3.forceManyBody().strength(-300))
      .force("center", d3.forceCenter(width / 2, height / 2))
      .force("collision", d3.forceCollide().radius(40))
      .force("x", d3.forceX(width / 2).strength(0.05))
      .force("y", d3.forceY(height / 2).strength(0.05));

    // Links
    const link = svg.append("g")
      .selectAll<SVGLineElement, GraphLink>("line")
      .data(links)
      .join("line")
      .attr("stroke", d => getLinkColor(d.link_type))
      .attr("stroke-opacity", 0.6)
      .attr("stroke-width", 2)
      .attr("stroke-dasharray", d => getLinkDash(d.link_type))
      .attr("marker-end", d => `url(#arrow-${d.link_type})`);

    // Link labels
    const linkLabel = svg.append("g")
      .selectAll<SVGTextElement, GraphLink>("text")
      .data(links)
      .join("text")
      .attr("font-size", "10px")
      .attr("fill", "hsl(var(--muted-foreground))")
      .attr("text-anchor", "middle")
      .text(d => d.link_type);

    // Node groups
    const node = svg.append("g")
      .selectAll<SVGGElement, GraphNode>("g")
      .data(nodes)
      .join("g")
      .call(d3.drag<SVGGElement, GraphNode>()
        .on("start", dragstarted)
        .on("drag", dragged)
        .on("end", dragended)
      );

    // Node circles
    node.append("circle")
      .attr("r", d => getNodeRadius(d.node_type))
      .attr("fill", d => d.online ? getNodeColor(d.node_type) : "#4b5563")
      .attr("stroke", d => getNodeColor(d.node_type))
      .attr("stroke-width", 2)
      .attr("opacity", d => d.online ? 1 : 0.5)
      .style("filter", d => d.node_type === "self" ? "url(#glow)" : "none");

    // Node icon
    node.append("text")
      .attr("text-anchor", "middle")
      .attr("dominant-baseline", "central")
      .attr("font-size", d => d.node_type === "self" ? "16px" : "12px")
      .attr("fill", "#fff")
      .text(d => getNodeIcon(d.node_type));

    // Node labels
    node.append("text")
      .attr("x", 0)
      .attr("y", d => getNodeRadius(d.node_type) + 14)
      .attr("text-anchor", "middle")
      .attr("font-size", "11px")
      .attr("fill", "hsl(var(--foreground))")
      .attr("font-family", "monospace")
      .text(d => d.label);

    // Status dot for online indicator
    node.filter(d => d.node_type === "peer")
      .append("circle")
      .attr("cx", d => getNodeRadius(d.node_type) - 2)
      .attr("cy", d => -(getNodeRadius(d.node_type) - 2))
      .attr("r", 4)
      .attr("fill", d => d.online ? "#22c55e" : "#ef4444")
      .attr("stroke", "hsl(var(--card))")
      .attr("stroke-width", 1.5);

    // Tooltip on hover
    node.append("title")
      .text(d => `${d.label}\n${d.node_type.toUpperCase()}${d.online ? "" : " (offline)"}\nID: ${d.id}`);

    simulation.on("tick", () => {
      link
        .attr("x1", d => (d.source as GraphNode).x!)
        .attr("y1", d => (d.source as GraphNode).y!)
        .attr("x2", d => (d.target as GraphNode).x!)
        .attr("y2", d => (d.target as GraphNode).y!);

      linkLabel
        .attr("x", d => ((d.source as GraphNode).x! + (d.target as GraphNode).x!) / 2)
        .attr("y", d => ((d.source as GraphNode).y! + (d.target as GraphNode).y!) / 2 - 6);

      node.attr("transform", d => `translate(${d.x},${d.y})`);
    });

    function dragstarted(event: d3.D3DragEvent<SVGGElement, GraphNode, GraphNode>) {
      if (!event.active) simulation.alphaTarget(0.3).restart();
      event.subject.fx = event.subject.x;
      event.subject.fy = event.subject.y;
    }

    function dragged(event: d3.D3DragEvent<SVGGElement, GraphNode, GraphNode>) {
      event.subject.fx = event.x;
      event.subject.fy = event.y;
    }

    function dragended(event: d3.D3DragEvent<SVGGElement, GraphNode, GraphNode>) {
      if (!event.active) simulation.alphaTarget(0);
      event.subject.fx = null;
      event.subject.fy = null;
    }
  }

  onMount(() => {
    renderGraph();

    const observer = new ResizeObserver(() => renderGraph());
    observer.observe(container);
    return () => observer.disconnect();
  });

  $effect(() => {
    // Re-render when data changes
    if (data && container) {
      renderGraph();
    }
  });
</script>

<div class="w-full rounded-lg bg-[hsl(var(--card))] border border-[hsl(var(--border))] overflow-hidden">
  <!-- Legend -->
  <div class="flex flex-wrap items-center gap-4 px-4 py-2 border-b border-[hsl(var(--border))] text-xs text-[hsl(var(--muted-foreground))]">
    <div class="flex items-center gap-1.5">
      <span class="inline-block w-3 h-3 rounded-full bg-green-500"></span>
      {t('admin.graph.selfNode')}
    </div>
    <div class="flex items-center gap-1.5">
      <span class="inline-block w-3 h-3 rounded-full bg-blue-500"></span>
      {t('admin.graph.relayNode')}
    </div>
    <div class="flex items-center gap-1.5">
      <span class="inline-block w-3 h-3 rounded-full bg-purple-500"></span>
      {t('admin.graph.peerNode')}
    </div>
    <div class="flex items-center gap-1.5 ml-auto">
      <span class="inline-block w-6 border-t-2 border-dashed border-blue-500"></span>
      relay
    </div>
    <div class="flex items-center gap-1.5">
      <span class="inline-block w-6 border-t-2 border-green-500"></span>
      direct
    </div>
  </div>

  <!-- Graph container -->
  <div bind:this={container} class="w-full" style="height: 500px;"></div>
</div>
