import { describe, expect, it } from "vitest";
import type { MemoryFieldProjection } from "../api";
import { layoutCategoryGraph } from "./graphLayout";

type Category = MemoryFieldProjection["categories"][number];
const node = (id: string, count = 10): Category => ({ id, name: id, kind: "domain", description: id, memory_count: count, child_count: 2, relation_count: 0 });

describe("layoutCategoryGraph", () => {
  it("is deterministic and keeps aggregate nodes inside the viewport", () => {
    const nodes = [node("a", 40), node("b", 20), node("c", 10)];
    const edges = [{ source: "a", target: "b", edge_type: "SUPPORTS", count: 3 }];
    const first = layoutCategoryGraph(nodes, edges, 900, 600);
    expect(layoutCategoryGraph(nodes, edges, 900, 600)).toEqual(first);
    expect(first.nodes.every(item => item.x >= 70 && item.x <= 830 && item.y >= 70 && item.y <= 530)).toBe(true);
  });

  it("folds parallel relations and emits a non-crossing backbone", () => {
    const nodes = [node("a"), node("b"), node("c"), node("d")];
    const result = layoutCategoryGraph(nodes, [
      { source: "a", target: "c", edge_type: "BECAUSE", count: 4 },
      { source: "a", target: "c", edge_type: "SUPPORTS", count: 2 },
      { source: "b", target: "d", edge_type: "IMPLIES", count: 3 },
    ]);
    expect(result.edges.length + result.hiddenEdgeCount).toBe(2);
    expect(result.edges.some(edge => edge.edge_type === "BECAUSE")).toBe(true);
  });

  it("keeps an intra-category relation so the UI can draw it as a loop", () => {
    const result = layoutCategoryGraph(
      [node("knowledge-graphs")],
      [{ source: "knowledge-graphs", target: "knowledge-graphs", edge_type: "IS_A", count: 7 }],
    );
    expect(result.edges).toEqual([
      { source: "knowledge-graphs", target: "knowledge-graphs", edge_type: "IS_A", count: 7 },
    ]);
    expect(result.hiddenEdgeCount).toBe(0);
  });
});
