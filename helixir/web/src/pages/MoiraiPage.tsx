import { useEffect, useMemo, useState } from "react";
import { apiPost, type HostOperationResult, type MoiraiProjection } from "../api";
import { StatusDot } from "../components";
import { PageState, useResource } from "./shared";

const PAGE_SIZE = 10;

export function MoiraiPage({ hostOperationsAvailable }: { hostOperationsAvailable: boolean }) {
  const { data, error, loading, refresh } = useResource<MoiraiProjection>("/moirai", { pollMs: 30_000 });
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const [daemonUser, setDaemonUser] = useState("codex");
  const [daemonInterval, setDaemonInterval] = useState("300");
  const [operation, setOperation] = useState<HostOperationResult | null>(null);
  const [operationProblem, setOperationProblem] = useState<string | null>(null);
  const [operating, setOperating] = useState(false);
  const insights = useMemo(() => data?.insights.filter(insight => `${insight.memory.content} ${insight.memory.context_tags} ${insight.memory.memory_type} ${insight.source_groups.join(" ")} ${insight.witnesses.map(witness => witness.content).join(" ")}`.toLowerCase().includes(query.toLowerCase())) ?? [], [data, query]);
  const pageCount = Math.max(1, Math.ceil(insights.length / PAGE_SIZE));
  const visible = insights.slice((Math.min(page, pageCount) - 1) * PAGE_SIZE, Math.min(page, pageCount) * PAGE_SIZE);
  useEffect(() => setPage(1), [query]);
  const runOperation = async (body: unknown) => {
    setOperating(true); setOperation(null); setOperationProblem(null);
    try { const result = await apiPost<HostOperationResult>("/operations/run", body); setOperation(result); refresh(); }
    catch (reason) { setOperationProblem(reason instanceof Error ? reason.message : "Moirai operation failed"); }
    finally { setOperating(false); }
  };

  return <div className="page-canvas section-page moirai-page">
    <div className="page-heading"><div><p className="eyebrow"><span>05</span> admin-only generative layer</p><h1>The Moirai</h1><p className="section-lede">Live monitoring for Clotho, Lachesis and Atropos, plus every durable hypothesis and the memories that justify it.</p></div><button className="ghost-action" onClick={refresh}>Refresh monitoring</button></div>
    <PageState loading={loading} error={error} />
    {data && <><section className="moirai-circuit">{data.stages.map((stage, index) => <article className={`moirai-agent is-${stage.state}`} key={stage.name}><header><span>0{index + 1}</span><StatusDot ok={stage.state !== "idle"} pulse={stage.state === "active"} /></header><strong>{stage.name}</strong><small>{stage.responsibility}</small><dl><div><dt>State</dt><dd>{stage.state}</dd></div><div><dt>Artifacts</dt><dd>{stage.artifact_count}</dd></div><div><dt>Last signal</dt><dd>{stage.last_activity_at ? new Date(stage.last_activity_at).toLocaleString() : "none"}</dd></div></dl></article>)}<div className="daemon-state"><StatusDot ok={data.daemon_active} pulse={data.daemon_active} /><strong>{data.daemon_active ? "Daemon awake" : data.enabled ? "Daemon standing by" : "Insights disabled"}</strong><span>{data.daemon_status ?? data.mode}</span><small>scheduler / orchestration</small></div></section>
      <section className="operations-desk"><header><div><p className="eyebrow">Scheduler control</p><h2>Moirai daemon</h2></div><span>{hostOperationsAvailable ? "Typed host bridge online" : "Host supervisor unavailable"}</span></header><div className="operation-form"><label>Memory owner<input onChange={event => setDaemonUser(event.target.value)} value={daemonUser} /></label><label>Interval, seconds<input inputMode="numeric" min="30" onChange={event => setDaemonInterval(event.target.value)} value={daemonInterval} /></label><button className="primary-action compact" disabled={!hostOperationsAvailable || operating || !daemonUser.trim()} onClick={() => void runOperation({ kind: "daemon_start", user_id: daemonUser.trim(), interval_secs: Number(daemonInterval) })}>Start daemon</button><button className="danger-action" disabled={!hostOperationsAvailable || operating} onClick={() => { if (window.confirm("Stop the Moirai daemon?")) void runOperation({ kind: "daemon_stop" }); }}>Stop</button></div>{operation && <pre className={operation.succeeded ? "operation-output is-ok" : "operation-output"}>{operation.output || `${operation.operation} completed`}</pre>}{operationProblem && <p className="inline-notice">{operationProblem}</p>}<p className="operation-note">Long-running one-pass pipelines stay in the CLI until resumable operations and reconnectable progress from #144 are complete; start/stop is safe and immediate here.</p></section>
      <section className="moirai-integrity"><article><span>Hypotheses</span><strong>{data.insights.length}</strong><small>durable Moirai memories</small></article><article><span>Witness links</span><strong>{data.witness_count}</strong><small>MOIRAI_DERIVED_FROM</small></article><article className={data.orphan_count ? "is-alert" : "is-ok"}><span>Orphans</span><strong>{data.orphan_count}</strong><small>{data.orphan_count ? "integrity violation" : "every hypothesis is grounded"}</small></article></section>
      <section className="moirai-journal"><header><div><p className="eyebrow">Insight journal</p><h2>{insights.length} durable hypotheses</h2></div><label className="search-box"><span>⌕</span><input aria-label="Search Moirai journal" onChange={event => setQuery(event.target.value)} placeholder="Search text, tag or workspace" value={query} /></label></header>
        {visible.length ? <div className="insight-accordion">{visible.map((insight, index) => <details className={insight.orphaned ? "is-orphan" : ""} key={insight.memory.id}><summary><span className="insight-index">{String((page - 1) * PAGE_SIZE + index + 1).padStart(2, "0")}</span><div><strong>{short(insight.memory.content, 105)}</strong><small><time>{insight.memory.created_at ? new Date(insight.memory.created_at).toLocaleString() : "undated"}</time><em>{insight.memory.context_tags || insight.memory.memory_type || "hypothesis"}</em></small></div><div className="insight-groups">{(insight.source_groups.length ? insight.source_groups : insight.memory.groups).map(group => <span key={group}>{group}</span>)}<b>{insight.orphaned ? "ORPHAN" : `${insight.witness_count} witnesses`}</b></div><i>＋</i></summary><article><p>{insight.memory.content}</p><section className="witness-ledger"><h3>Evidence ledger</h3>{insight.witnesses.length ? insight.witnesses.map((witness, witnessIndex) => <div key={witness.id}><b>{String(witnessIndex + 1).padStart(2, "0")}</b><p>{witness.content}</p><span>{witness.user_id || "system"} · {witness.groups.join(" / ") || "unscoped"}</span><code>{witness.id}</code></div>) : <div className="orphan-warning"><strong>No witness provenance persisted</strong><p>This hypothesis must be investigated before it is treated as a usable lead.</p></div>}</section><footer><span>Owner / {insight.memory.user_id || "helixir"}</span><span>Source / {insight.memory.source || "moirai"}</span><code>{insight.memory.id}</code></footer></article></details>)}</div> : <div className="empty-insights"><span>∴</span><h2>No matching hypotheses</h2><p>The Moirai journal has no entries for this filter.</p></div>}
        <footer className="pagination"><span>{insights.length} records</span><div><button disabled={page <= 1} onClick={() => setPage(page - 1)}>Previous</button><b>{Math.min(page, pageCount)} / {pageCount}</b><button disabled={page >= pageCount} onClick={() => setPage(page + 1)}>Next</button></div></footer>
      </section>
    </>}
  </div>;
}

function short(value: string, max: number) { return value.length > max ? `${value.slice(0, max)}…` : value; }
