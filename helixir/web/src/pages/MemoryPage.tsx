import { useMemo, useRef, useState, type FormEvent } from "react";
import type { MemoryFieldProjection, MemoryProjection } from "../api";
import { PageState, useResource } from "./shared";
import { layoutCategoryGraph, type CategoryNode } from "./graphLayout";

const edgeColors: Record<string, string> = {
  BECAUSE: "#ffb547", IMPLIES: "#54b7d9", SUPPORTS: "#8c7dff", RELATES_TO: "#b9aefc",
  CONTRADICTS: "#ef765f", IS_A: "#b5d45c", PART_OF: "#d88ccf", MOIRAI_DERIVED_FROM: "#f3df75",
};
const nodeColors = ["#ffb547", "#8c7dff", "#62d6a6", "#54b7d9", "#d88ccf", "#ef765f", "#b5d45c"];
const canonicalRelations = ["BECAUSE", "IMPLIES", "SUPPORTS", "CONTRADICTS", "RELATES_TO", "IS_A", "PART_OF", "MOIRAI_DERIVED_FROM"];
const snapshotRefreshMs = 5 * 60 * 1000;

export function MemoryPage() {
  const [group, setGroup] = useState("all");
  const [identity, setIdentity] = useState("all");
  const [identityDraft, setIdentityDraft] = useState("");
  const [focus, setFocus] = useState("all");
  const [query, setQuery] = useState("");
  const [queryDraft, setQueryDraft] = useState("");
  const [page, setPage] = useState(1);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [edgeType, setEdgeType] = useState("ALL");
  const identityInput = useRef<HTMLInputElement | null>(null);
  const categoryInput = useRef<HTMLInputElement | null>(null);
  const path = `/memory-field?group=${encodeURIComponent(group)}&identity=${encodeURIComponent(identity)}&focus=${encodeURIComponent(focus)}&query=${encodeURIComponent(query)}&page=${page}`;
  const { data, error, loading, refresh } = useResource<MemoryFieldProjection>(path, { pollMs: snapshotRefreshMs });
  const relationTypes = useMemo(() => relationPalette(data?.category_edges ?? [], data?.relation_totals ?? []), [data]);
  const visibleEdges = useMemo(() => data?.category_edges.filter(edge => edgeType === "ALL" || edge.edge_type === edgeType) ?? [], [data, edgeType]);
  const layout = useMemo(() => layoutCategoryGraph(data?.categories ?? [], visibleEdges), [data, visibleEdges]);
  const positions = useMemo(() => new Map(layout.nodes.map(item => [item.node.id, item])), [layout]);
  const selected = data?.categories.find(node => node.id === selectedId) ?? null;
  const selectedRelations = selected ? data?.category_edges.filter(edge => edge.source === selected.id || edge.target === selected.id) ?? [] : [];
  const openFocus = (id: string) => { setFocus(id); setQuery(""); setQueryDraft(""); setPage(1); setSelectedId(null); setEdgeType("ALL"); };
  const resetAtlas = () => openFocus("all");
  const applyIdentity = (event: FormEvent) => {
    event.preventDefault();
    const candidate = identityDraft.trim();
    const valid = data?.identities.some(item => `${item.kind}:${item.identity}` === candidate);
    setIdentity(valid ? candidate : "all"); resetAtlas();
  };
  const applyQuery = (event: FormEvent) => {
    event.preventDefault();
    setQuery(queryDraft.trim());
    setPage(1);
    setSelectedId(null);
  };
  return <div className="page-canvas section-page memory-page">
    <div className="page-heading"><div><p className="eyebrow"><span>04</span> asynchronous category atlas</p><h1>Memory field</h1><p className="section-lede">Real categories first; governed memories load only when an administrator opens a category.</p></div><button className="ghost-action" onClick={refresh}>Refresh cached view</button></div>
    <PageState loading={loading} error={error} />
    {data && <>
      <section className="graph-summary category-summary">
        <button onClick={resetAtlas} type="button"><strong>{data.total_memories.toLocaleString()}</strong><span>memories in current security scope</span></button>
        <button onClick={() => setEdgeType("ALL")} type="button"><strong>{data.relation_totals.reduce((sum, edge) => sum + edge.count, 0).toLocaleString()}</strong><span>real typed memory relations</span></button>
        <button onClick={() => data.view === "categories" ? categoryInput.current?.focus() : identityInput.current?.focus()} type="button"><strong>{data.view === "categories" ? data.total_categories : data.memories.length}</strong><span>{data.view === "memories" ? "memories on this page" : "real categories in this level"}</span></button>
        <label>Workspace<select onChange={event => { setGroup(event.target.value); resetAtlas(); }} value={group}><option value="all">All visible groups</option>{data.groups.map(item => <option key={item.group_id} value={item.group_id}>{item.name} / {item.group_id}</option>)}</select></label>
        <form className="identity-filter" onSubmit={applyIdentity}><label>Identity<input aria-label="Filter category atlas by identity" list="memory-identities" onChange={event => setIdentityDraft(event.target.value)} placeholder="user_id or agent_id" ref={identityInput} value={identityDraft} /><datalist id="memory-identities">{data.identities.map(item => <option key={`${item.kind}:${item.identity}`} value={`${item.kind}:${item.identity}`} />)}</datalist></label><button type="submit">Apply</button>{identity !== "all" && <button onClick={() => { setIdentity("all"); setIdentityDraft(""); resetAtlas(); }} type="button">Clear</button>}</form>
      </section>
      <nav className="atlas-breadcrumbs" aria-label="Category location">{data.breadcrumbs.map((item, index) => <button aria-current={index === data.breadcrumbs.length - 1 ? "page" : undefined} disabled={index === data.breadcrumbs.length - 1} key={item.id} onClick={() => openFocus(item.id)}>{item.name}</button>)}</nav>
      {focus === "all" && data.uncategorized_memories > 0 && <button className="uncategorized-backlog" onClick={() => openFocus("atlas:uncategorized")}><span>Classification backlog</span><strong>{data.uncategorized_memories.toLocaleString()} memories</strong><em>Open journal →</em></button>}
      {data.view === "memories"
        ? <MemoryJournal data={data} onPage={setPage} />
        : <><div className="category-workbench">
          <section className="category-canvas">
            <header><div><p className="eyebrow">Five-minute materialized view</p><span>Snapshot {new Date(data.snapshot_at).toLocaleTimeString()} · next {new Date(data.next_refresh_at).toLocaleTimeString()}</span></div><form className="category-search" onSubmit={applyQuery}><input aria-label="Search categories" onChange={event => setQueryDraft(event.target.value)} placeholder="Search categories" ref={categoryInput} value={queryDraft} /><button type="submit">Find</button>{query && <button onClick={() => { setQuery(""); setQueryDraft(""); setPage(1); }} type="button">Clear</button>}</form><span className="graph-static-note">Fixed atlas · click nodes to inspect</span></header>
            <div className="relation-filter"><button aria-pressed={edgeType === "ALL"} className={edgeType === "ALL" ? "is-active" : ""} onClick={() => setEdgeType("ALL")}><i style={{ background: "#f4efe6" }} />All <b>{data.category_edges.length}</b></button>{relationTypes.map(item => <button aria-pressed={edgeType === item.type} className={edgeType === item.type ? "is-active" : ""} disabled={item.visible === 0} key={item.type} onClick={() => setEdgeType(item.type)} title={item.visible ? `${item.visible} category projections on this page` : `${item.count} real relations; none map between categories on this page`}><i style={{ background: edgeColor(item.type) }} />{item.type} <b>{item.count}</b></button>)}</div>
            <svg aria-label="Fixed-layout clickable category graph" onClick={() => setSelectedId(null)} viewBox="0 0 1000 650"><defs>{relationTypes.map(item => <marker id={`category-arrow-${item.type}`} key={item.type} markerHeight="7" markerWidth="7" orient="auto" refX="7" refY="3.5"><path d="M0,0 L7,3.5 L0,7 Z" fill={edgeColor(item.type)} /></marker>)}</defs><g>
              <g className="category-edges">{layout.edges.map(edge => { const source = positions.get(edge.source), target = positions.get(edge.target); if (!source || !target) return null; const color = edgeColor(edge.edge_type), width = Math.min(5, 1.6 + Math.log2(edge.count + 1) * .55); if (edge.source === edge.target) { const radius = source.radius + 11; return <g key={`${edge.source}/${edge.target}/${edge.edge_type}`}><title>{edge.edge_type} · {edge.count}</title><path d={`M ${source.x - radius * .7} ${source.y - radius * .7} C ${source.x - radius * 1.8} ${source.y - radius * 2.1}, ${source.x + radius * 1.8} ${source.y - radius * 2.1}, ${source.x + radius * .7} ${source.y - radius * .7}`} markerEnd={`url(#category-arrow-${edge.edge_type})`} style={{ stroke: color, strokeWidth: width }} /></g>; } return <g key={`${edge.source}/${edge.target}/${edge.edge_type}`}><title>{edge.edge_type} · {edge.count}</title><line markerEnd={`url(#category-arrow-${edge.edge_type})`} style={{ stroke: color, strokeWidth: width }} x1={source.x} x2={target.x} y1={source.y} y2={target.y} /></g>; })}</g>
              <g>{layout.nodes.map((item, index) => <g className={`category-node${selectedId === item.node.id ? " is-selected" : ""}`} key={item.node.id} onClick={event => { event.stopPropagation(); setSelectedId(item.node.id); }} onDoubleClick={() => openFocus(item.node.id)} transform={`translate(${item.x} ${item.y})`}><circle className="category-halo" r={item.radius + 8} style={{ stroke: nodeColors[index % nodeColors.length] }} /><circle r={item.radius} style={{ fill: nodeColors[index % nodeColors.length] }} /><text className="category-count" y="-3">{item.node.memory_count}</text><text className="category-name" y="22">{short(item.node.name, 17)}</text></g>)}</g>
            </g></svg>
            <footer><span>{layout.edges.length} strongest non-crossing links shown</span>{layout.hiddenEdgeCount > 0 && <em>{layout.hiddenEdgeCount} folded links remain in the inspector</em>}<b>Click to inspect · double-click to open</b></footer>
          </section>
          <CategoryInspector node={selected} relations={selectedRelations} nodes={data.categories} onOpen={openFocus} />
        </div><CategoryPager data={data} onPage={page => { setPage(page); setSelectedId(null); }} /></>}
    </>}
  </div>;
}

function CategoryPager({ data, onPage }: { data: MemoryFieldProjection; onPage: (page: number) => void }) {
  return <footer className="pagination category-pagination"><span>{data.total_categories} real categories · {data.page_size} per view</span><div><button disabled={data.page <= 1} onClick={() => onPage(data.page - 1)}>Previous</button><b>{data.page} / {data.page_count}</b><button disabled={data.page >= data.page_count} onClick={() => onPage(data.page + 1)}>Next</button></div></footer>;
}

function CategoryInspector({ node, relations, nodes, onOpen }: { node: CategoryNode | null; relations: MemoryFieldProjection["category_edges"]; nodes: CategoryNode[]; onOpen: (id: string) => void }) {
  if (!node) return <aside className="category-inspector"><div className="empty-inspector"><span>◉</span><strong>Select a category</strong><p>Inspect its memory volume and complete typed relation ledger. Double-click a circle to descend immediately.</p></div></aside>;
  const byId = new Map(nodes.map(item => [item.id, item]));
  const kinds = relationSummary(relations);
  return <aside className="category-inspector"><p className="eyebrow">{node.kind} cluster</p><h2>{node.name}</h2><p className="category-description">{node.description || "Controlled Helixir category."}</p><section className="connection-stats"><article><strong>{node.memory_count}</strong><span>memories</span></article><article><strong>{node.child_count}</strong><span>subcategories</span></article><article><strong>{node.relation_count}</strong><span>relations</span></article></section><button className="primary-action atlas-open" onClick={() => onOpen(node.id)}>Open {node.child_count ? "category group" : "memory journal"}<span>→</span></button><section className="relation-ledger"><h3>Complete relation ledger</h3>{kinds.map(kind => <div key={kind.type}><i style={{ background: edgeColor(kind.type) }} /><strong>{kind.type}</strong><span>{kind.count}</span></div>)}</section><section className="relation-list"><h3>Connected categories</h3>{relations.sort((a, b) => b.count - a.count).slice(0, 14).map(edge => { const peerId = edge.source === node.id ? edge.target : edge.source; return <button key={`${peerId}/${edge.edge_type}`} onClick={() => onOpen(peerId)}><em>{edge.edge_type} · {edge.count}</em><span>{byId.get(peerId)?.name ?? peerId}</span></button>; })}{!relations.length && <p>No cross-category reasoning edges in this snapshot.</p>}</section></aside>;
}

function MemoryJournal({ data, onPage }: { data: MemoryFieldProjection; onPage: (page: number) => void }) {
  const byId = new Map(data.memories.map(memory => [memory.id, memory]));
  return <section className="category-memory-journal"><header><div><p className="eyebrow">Category contents</p><h2>{data.total_memories.toLocaleString()} governed memories</h2></div><span>Page {data.page} / {data.page_count}</span></header><div>{data.memories.map(memory => <MemoryPanel edges={data.memory_edges} key={memory.id} memory={memory} peers={byId} />)}</div><footer className="pagination"><span>{data.total_memories} records</span><div><button disabled={data.page <= 1} onClick={() => onPage(data.page - 1)}>Previous</button><b>{data.page} / {data.page_count}</b><button disabled={data.page >= data.page_count} onClick={() => onPage(data.page + 1)}>Next</button></div></footer></section>;
}

function MemoryPanel({ memory, edges, peers }: { memory: MemoryProjection; edges: MemoryFieldProjection["memory_edges"]; peers: Map<string, MemoryProjection> }) {
  const relations = edges.filter(edge => edge.source === memory.id || edge.target === memory.id);
  return <details className="category-memory-panel"><summary><span className="type-badge">{memory.memory_type}</span><strong>{short(memory.content, 120)}</strong><div><b>{relations.length} links</b><time>{memory.created_at ? new Date(memory.created_at).toLocaleDateString() : "unknown"}</time></div><i>＋</i></summary><article><p>{memory.content}</p><dl><div><dt>Owner</dt><dd>{memory.user_id || "system"}</dd></div><div><dt>Groups</dt><dd>{memory.groups.join(" · ") || "admin-only"}</dd></div><div><dt>Scope</dt><dd>{memory.rbac_scope || "compatibility"}</dd></div><div><dt>Source</dt><dd>{memory.source || "memory"}</dd></div></dl>{relations.length > 0 && <section><h3>Relations on this page</h3>{relations.map((edge, index) => { const peer = peers.get(edge.source === memory.id ? edge.target : edge.source); return <div key={`${edge.source}/${edge.target}/${index}`}><i style={{ background: edgeColor(edge.edge_type) }} /><b>{edge.edge_type}</b><span>{peer ? short(peer.content, 92) : "Connected memory outside this page"}</span></div>; })}</section>}<code>{memory.id}</code></article></details>;
}

function relationSummary(edges: MemoryFieldProjection["category_edges"]) {
  const counts = new Map<string, number>();
  edges.forEach(edge => counts.set(edge.edge_type, (counts.get(edge.edge_type) ?? 0) + edge.count));
  return [...counts].map(([type, count]) => ({ type, count })).sort((a, b) => b.count - a.count || a.type.localeCompare(b.type));
}
function relationPalette(edges: MemoryFieldProjection["category_edges"], totals: MemoryFieldProjection["relation_totals"]) {
  const visible = new Map(relationSummary(edges).map(item => [item.type, item.count]));
  const global = new Map(totals.map(item => [item.edge_type, item.count]));
  const types = [...canonicalRelations, ...[...global.keys()].filter(type => !canonicalRelations.includes(type)).sort()];
  return types.map(type => ({ type, count: global.get(type) ?? 0, visible: visible.get(type) ?? 0 }));
}
function edgeColor(type: string) { return edgeColors[type] ?? "#a7a0bb"; }
function short(value: string, max: number) { return value.length > max ? `${value.slice(0, max)}…` : value; }
