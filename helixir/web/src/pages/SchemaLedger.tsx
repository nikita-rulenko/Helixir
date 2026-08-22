import { useMemo, useState } from "react";

import type { SchemaInventoryReport, SchemaKind, SchemaLifecycle } from "../api";
import { PageState, useResource } from "./shared";

const kinds: Array<{ id: SchemaKind; label: string; glyph: string }> = [
  { id: "node", label: "Nodes", glyph: "N" },
  { id: "vector", label: "Vectors", glyph: "V" },
  { id: "edge", label: "Edges", glyph: "E" },
];

export function SchemaLedger() {
  const schema = useResource<SchemaInventoryReport>("/schema", { pollMs: 300_000 });
  const [kind, setKind] = useState<SchemaKind>("node");
  const [lifecycle, setLifecycle] = useState<SchemaLifecycle | "all">("all");
  const visible = useMemo(() => schema.data?.items.filter(item => (
    item.kind === kind && (lifecycle === "all" || item.lifecycle === lifecycle)
  )) ?? [], [schema.data, kind, lifecycle]);

  return <section className="schema-ledger" aria-label="Physical schema lifecycle">
    <header className="schema-ledger-head">
      <div><p className="eyebrow">Physical graph contract</p><h2>Schema ledger</h2><p>What HelixDB stores, what Helixir actively uses, and what remains deliberately dormant.</p></div>
      <button className="ghost-action" onClick={schema.refresh} type="button">Refresh census</button>
    </header>
    <PageState loading={schema.loading} error={schema.error} />
    {schema.data && <>
      {schema.data.failed_queries.length > 0 && <p className="schema-warning" role="alert">Census is incomplete. Deploy the current HelixDB schema; unavailable queries: {schema.data.failed_queries.join(", ")}.</p>}
      <div className="schema-totals" aria-label="Schema lifecycle totals">
        <LifecycleTotal label="Active" value={schema.data.active} tone="active" onClick={() => setLifecycle("active")} />
        <LifecycleTotal label="Reserved" value={schema.data.reserved} tone="reserved" onClick={() => setLifecycle("reserved")} />
        <LifecycleTotal label="Deprecated" value={schema.data.deprecated} tone="deprecated" onClick={() => setLifecycle("deprecated")} />
        <button className="schema-total is-neutral" type="button" onClick={() => setLifecycle("all")}><span>Census</span><strong>{schema.data.counted}/{schema.data.items.length}</strong><small>server-side scalars</small></button>
      </div>
      <div className="schema-toolbar">
        <div className="schema-kinds" role="tablist" aria-label="Schema families">{kinds.map(entry => <button key={entry.id} role="tab" aria-selected={kind === entry.id} className={kind === entry.id ? "is-active" : ""} onClick={() => setKind(entry.id)} type="button"><b>{entry.glyph}</b>{entry.label}<span>{schema.data!.items.filter(item => item.kind === entry.id).length}</span></button>)}</div>
        <span>inventory v{schema.data.inventory_version} · {lifecycle === "all" ? "all lifecycle states" : lifecycle}</span>
      </div>
      <div className="schema-records">{visible.map(item => <details key={`${item.kind}-${item.name}`} className={`schema-record is-${item.lifecycle}`}>
        <summary><span className="schema-sigil">{item.kind[0].toUpperCase()}</span><div><strong>{item.name}</strong><small>{item.purpose}</small></div><em>{item.lifecycle}</em><b>{item.count ?? "—"}</b></summary>
        <div className="schema-record-body"><dl><div><dt>Owner</dt><dd>{item.owner}</dd></div>{item.producer && <div><dt>Producer</dt><dd>{item.producer}</dd></div>}{item.consumer && <div><dt>Consumer</dt><dd>{item.consumer}</dd></div>}{item.e2e && <div><dt>DB proof</dt><dd>{item.e2e}</dd></div>}{item.milestone && <div><dt>Milestone</dt><dd>{item.milestone}</dd></div>}{item.migration && <div className="is-wide"><dt>Migration</dt><dd>{item.migration}</dd></div>}</dl></div>
      </details>)}</div>
    </>}
  </section>;
}

function LifecycleTotal({ label, value, tone, onClick }: { label: string; value: number; tone: SchemaLifecycle; onClick: () => void }) {
  return <button className={`schema-total is-${tone}`} type="button" onClick={onClick}><span>{label}</span><strong>{value}</strong><small>{tone === "active" ? "producer + consumer + DB proof" : tone === "reserved" ? "owned future contract" : "migration required"}</small></button>;
}
