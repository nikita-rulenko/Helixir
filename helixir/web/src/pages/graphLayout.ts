import type { MemoryFieldProjection } from "../api";

export type CategoryNode = MemoryFieldProjection["categories"][number];
export type CategoryEdge = MemoryFieldProjection["category_edges"][number];

export interface PositionedCategory {
  node: CategoryNode;
  x: number;
  y: number;
  radius: number;
}

export interface CategoryLayout {
  nodes: PositionedCategory[];
  edges: CategoryEdge[];
  hiddenEdgeCount: number;
}

/**
 * Place category aggregates on a stable ellipse and keep only the strongest
 * non-crossing relational backbone. The inspector still exposes every folded
 * edge, so visual calm never means data loss.
 */
export function layoutCategoryGraph(
  nodes: CategoryNode[],
  edges: CategoryEdge[],
  width = 1000,
  height = 650,
): CategoryLayout {
  if (!nodes.length) return { nodes: [], edges: [], hiddenEdgeCount: 0 };
  const allCompactEdges = strongestPerPair(edges);
  const showLoops = new Set(edges.map(edge => edge.edge_type)).size === 1;
  const compactEdges = allCompactEdges.filter(edge => edge.source !== edge.target || showLoops);
  const ordered = minimizeCrossings(nodes, compactEdges);
  const maximum = Math.max(...nodes.map(node => node.memory_count), 1);
  const positioned = ordered.map((node, index) => {
    const angle = ordered.length === 1 ? 0 : index / ordered.length * Math.PI * 2 - Math.PI / 2;
    const radius = 28 + Math.sqrt(node.memory_count / maximum) * 24;
    return {
      node,
      radius,
      x: ordered.length === 1 ? width / 2 : width / 2 + Math.cos(angle) * Math.min(width * .36, 380),
      y: ordered.length === 1 ? height / 2 : height / 2 + Math.sin(angle) * Math.min(height * .34, 225),
    };
  });
  const positions = new Map(positioned.map(item => [item.node.id, item]));
  const accepted: CategoryEdge[] = [];
  const crossEdgeBudget = Math.max(8, Math.min(18, Math.ceil(nodes.length * .75)));
  let acceptedCrossEdges = 0;
  for (const edge of [...compactEdges].sort((a, b) => b.count - a.count || edgeKey(a).localeCompare(edgeKey(b)))) {
    const source = positions.get(edge.source), target = positions.get(edge.target);
    if (!source || !target || segmentHitsNode(source, target, positioned)) continue;
    if (edge.source !== edge.target && acceptedCrossEdges >= crossEdgeBudget) continue;
    if (accepted.some(other => segmentsCross(source, target, positions.get(other.source)!, positions.get(other.target)!))) continue;
    accepted.push(edge);
    if (edge.source !== edge.target) acceptedCrossEdges += 1;
  }
  return { nodes: positioned, edges: accepted, hiddenEdgeCount: allCompactEdges.length - accepted.length };
}

function strongestPerPair(edges: CategoryEdge[]) {
  const strongest = new Map<string, CategoryEdge>();
  for (const edge of edges) {
    const key = [edge.source, edge.target].sort().join("\u0000");
    const current = strongest.get(key);
    if (!current || edge.count > current.count || (edge.count === current.count && edge.edge_type < current.edge_type)) strongest.set(key, edge);
  }
  return [...strongest.values()];
}

function minimizeCrossings(nodes: CategoryNode[], edges: CategoryEdge[]) {
  const degree = new Map(nodes.map(node => [node.id, 0]));
  edges.forEach(edge => {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + edge.count);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + edge.count);
  });
  const order = [...nodes].sort((a, b) => (degree.get(b.id) ?? 0) - (degree.get(a.id) ?? 0) || a.name.localeCompare(b.name));
  let best = circularCrossings(order, edges);
  for (let pass = 0; pass < 4; pass += 1) {
    let improved = false;
    for (let left = 0; left < order.length; left += 1) {
      for (let right = left + 1; right < order.length; right += 1) {
        [order[left], order[right]] = [order[right], order[left]];
        const score = circularCrossings(order, edges);
        if (score < best) { best = score; improved = true; }
        else [order[left], order[right]] = [order[right], order[left]];
      }
    }
    if (!improved) break;
  }
  return order;
}

function circularCrossings(nodes: CategoryNode[], edges: CategoryEdge[]) {
  const order = new Map(nodes.map((node, index) => [node.id, index]));
  let crossings = 0;
  for (let left = 0; left < edges.length; left += 1) for (let right = left + 1; right < edges.length; right += 1) {
    const a = edges[left], b = edges[right];
    if ([a.source, a.target].some(id => id === b.source || id === b.target)) continue;
    const [a1, a2] = [order.get(a.source), order.get(a.target)].sort(numberSort) as [number, number];
    const [b1, b2] = [order.get(b.source), order.get(b.target)].sort(numberSort) as [number, number];
    if ((a1 < b1 && b1 < a2 && a2 < b2) || (b1 < a1 && a1 < b2 && b2 < a2)) crossings += 1;
  }
  return crossings;
}

function segmentHitsNode(source: PositionedCategory, target: PositionedCategory, nodes: PositionedCategory[]) {
  return nodes.some(node => node !== source && node !== target && pointSegmentDistance(node, source, target) < node.radius + 12);
}

function pointSegmentDistance(point: PositionedCategory, a: PositionedCategory, b: PositionedCategory) {
  const dx = b.x - a.x, dy = b.y - a.y;
  const t = Math.max(0, Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / Math.max(1, dx * dx + dy * dy)));
  return Math.hypot(point.x - (a.x + t * dx), point.y - (a.y + t * dy));
}

function segmentsCross(a: PositionedCategory, b: PositionedCategory, c: PositionedCategory, d: PositionedCategory) {
  if ([a.node.id, b.node.id].some(id => id === c.node.id || id === d.node.id)) return false;
  return orientation(a, b, c) * orientation(a, b, d) < 0 && orientation(c, d, a) * orientation(c, d, b) < 0;
}

function orientation(a: PositionedCategory, b: PositionedCategory, c: PositionedCategory) {
  return Math.sign((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x));
}

function edgeKey(edge: CategoryEdge) { return `${edge.source}/${edge.target}/${edge.edge_type}`; }
function numberSort(a: number | undefined, b: number | undefined) { return (a ?? -1) - (b ?? -1); }
