import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  apiEventStream,
  apiGet,
  apiPost,
  type DoctorReport,
  type InstallOptions,
  type InstallPlan,
  type OperationEvent,
  type OperationSnapshot,
} from "../api";
import { StatusDot } from "../components";

export const activeInstallOperationKey = "helixir.active-install-operation";

type Phase = "review" | "applying" | "interrupted" | "applied" | "verifying" | "complete";

export function InstallOperation({ plan, options, initialOperation, onBack, onStage }: {
  plan: InstallPlan;
  options: InstallOptions | null;
  initialOperation: OperationSnapshot | null;
  onBack: () => void;
  onStage: (stage: number) => void;
}) {
  const compatibleInitial = initialOperation && samePlan(initialOperation.plan, plan) ? initialOperation : null;
  const [phase, setPhase] = useState<Phase>(() => phaseFrom(compatibleInitial));
  const [operation, setOperation] = useState<OperationSnapshot | null>(compatibleInitial);
  const [events, setEvents] = useState<OperationEvent[]>(compatibleInitial?.events ?? []);
  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [problem, setProblem] = useState<string | null>(compatibleInitial?.error ?? null);
  const streamAbort = useRef<AbortController | null>(null);

  const hydrate = useCallback((snapshot: OperationSnapshot) => {
    setOperation(snapshot);
    setEvents(snapshot.events);
    setProblem(snapshot.error);
    sessionStorage.setItem(activeInstallOperationKey, snapshot.operation_id);
    if (snapshot.status === "succeeded") {
      setPhase("applied");
      onStage(4);
    } else if (snapshot.status === "failed" || snapshot.status === "interrupted") {
      setPhase("interrupted");
      onStage(3);
    } else {
      setPhase("applying");
      onStage(3);
    }
  }, [onStage]);

  const follow = useCallback(async (snapshot: OperationSnapshot) => {
    streamAbort.current?.abort();
    const controller = new AbortController();
    streamAbort.current = controller;
    let cursor = snapshot.events.at(-1)?.sequence ?? 0;
    hydrate(snapshot);
    while (!controller.signal.aborted) {
      try {
        cursor = await apiEventStream(
          `/install/operations/${snapshot.operation_id}/events`,
          cursor,
          event => setEvents(current => current.some(item => item.sequence === event.sequence)
            ? current
            : [...current, event]),
          controller.signal,
        );
        const latest = await apiGet<OperationSnapshot>(`/install/operations/${snapshot.operation_id}`);
        hydrate(latest);
        if (["succeeded", "failed", "interrupted"].includes(latest.status)) return;
      } catch (reason) {
        if (controller.signal.aborted) return;
        setProblem(reason instanceof Error ? `${reason.message}. Reconnecting…` : "Operation stream disconnected. Reconnecting…");
        await new Promise(resolve => window.setTimeout(resolve, 800));
      }
    }
  }, [hydrate]);

  useEffect(() => {
    if (compatibleInitial?.status === "queued" || compatibleInitial?.status === "running") {
      void follow(compatibleInitial);
    } else if (compatibleInitial) {
      hydrate(compatibleInitial);
    } else if (initialOperation) {
      sessionStorage.removeItem(activeInstallOperationKey);
    }
    return () => streamAbort.current?.abort();
  }, [compatibleInitial, follow, hydrate, initialOperation]);

  const apply = async () => {
    if (!options) {
      setProblem("Re-enter the installation choices so Helixir can safely revalidate the plan. Secrets are never stored in the operation journal.");
      return;
    }
    setPhase("applying");
    onStage(3);
    setProblem(null);
    try {
      const path = operation?.resumable
        ? `/install/operations/${operation.operation_id}/resume`
        : "/install/operations";
      const snapshot = await apiPost<OperationSnapshot>(path, options);
      await follow(snapshot);
    } catch (reason) {
      setPhase(operation?.resumable ? "interrupted" : "review");
      onStage(operation?.resumable ? 3 : 2);
      setProblem(reason instanceof Error ? reason.message : "Installation operation failed");
    }
  };

  const verify = async () => {
    setPhase("verifying");
    setProblem(null);
    try {
      const result = await apiPost<DoctorReport>("/install/verify", {});
      setDoctor(result);
      if (!result.ready) throw new Error("Doctor found required components that are not ready");
      setPhase("complete");
      onStage(4);
      sessionStorage.removeItem(activeInstallOperationKey);
    } catch (reason) {
      setPhase("applied");
      setProblem(reason instanceof Error ? reason.message : "Verification failed");
    }
  };

  const stepStates = useMemo(() => plan.steps.map((_, index) => {
    const report = operation?.report?.steps[index];
    if (report) return report.succeeded ? "done" : "failed";
    const event = [...events].reverse().find(item => item.install?.step_index === index);
    if (event?.install?.kind === "step_succeeded") return "done";
    if (event?.install?.kind === "step_failed") return "failed";
    if (event?.install?.kind === "step_started") return "running";
    return "waiting";
  }), [events, operation?.report, plan.steps]);
  const done = stepStates.filter(state => state === "done").length;
  const progress = plan.steps.length ? Math.round((done / plan.steps.length) * 100) : 0;
  const locked = phase === "applying" || phase === "verifying";

  return (
    <div className="inspection-card plan-card">
      <div className="plan-heading"><div><p className="eyebrow">{phaseLabel(phase)}</p><h2>{phase === "complete" ? "Ready for agents" : `${plan.steps.length} exact steps`}</h2></div><button disabled={locked} onClick={onBack} type="button">← Change choices</button></div>
      <section className="plan-explainer"><article className={phase === "review" ? "is-current" : ""}><b>1</b><div><strong>Review</strong><span>Nothing changes until you approve the exact plan.</span></div></article><article className={["applying", "interrupted", "applied"].includes(phase) ? "is-current" : ""}><b>2</b><div><strong>Apply</strong><span>Progress is journaled on the host and survives this page.</span></div></article><article className={["verifying", "complete"].includes(phase) ? "is-current" : ""}><b>3</b><div><strong>Verify</strong><span>Doctor proves database, models, RBAC and clients are ready.</span></div></article></section>
      {operation && <div className="operation-progress"><div><span>{operation.operation_id}</span><b>{operation.status}</b></div><progress max="100" value={operation.status === "succeeded" ? 100 : progress}>{progress}%</progress><small>{done} of {plan.steps.length} steps complete · event cursor {events.at(-1)?.sequence ?? 0}</small></div>}
      <details className="install-help"><summary>What should I do on this screen? <i>＋</i></summary><div><p>Review the plan, then apply it. You may reload or close this page: the native supervisor continues and the UI reconnects from the last event. Interrupted and failed attempts can resume only after the same plan is rebuilt; credentials are deliberately never written to the journal.</p><a href="https://github.com/nikita-rulenko/Helixir#one-command-install" rel="noreferrer" target="_blank">Open installation documentation ↗</a></div></details>
      <ol className="plan-list">{plan.steps.map((step, index) => <li className={`is-${stepStates[index]}`} key={`${step.action.kind}-${index}`}><span>{stepStates[index] === "done" ? "✓" : stepStates[index] === "failed" ? "×" : String(index + 1).padStart(2, "0")}</span><div><strong>{actionLabel(step.action.kind)}</strong><small>{operation?.report?.steps[index]?.detail ?? step.reason}</small></div><em>{stepStates[index] === "waiting" ? step.required ? "REQUIRED" : "OPTIONAL" : stepStates[index].toUpperCase()}</em></li>)}</ol>
      {events.length > 0 && <details className="operation-journal"><summary>Operation journal · {events.length} events <i>＋</i></summary><ol>{events.slice(-12).reverse().map(event => <li key={event.event_id}><time>{new Date(event.at).toLocaleTimeString()}</time><strong>{event.step_id ?? event.kind}</strong><span>{event.install?.action ? actionLabel(event.install.action.kind) : event.detail ?? event.kind}</span></li>)}</ol></details>}
      {doctor && <div className="doctor-results">{doctor.checks.map(check => <span className={`is-${check.status}`} key={check.name}><StatusDot ok={check.status === "pass" || check.status === "warn"} />{check.name}<b>{check.status}</b></span>)}</div>}
      {problem && <p className="form-problem operation-problem" role="alert">{problem}</p>}
      <div className="plan-lock"><StatusDot ok={phase === "complete" || phase === "applied"} pulse={locked} /><p>{phaseCopy(phase, Boolean(options))}</p>{phase === "review" && <button className="primary-action" onClick={() => void apply()}>Apply exact plan <span>→</span></button>}{phase === "interrupted" && options && <button className="primary-action" onClick={() => void apply()}>Revalidate &amp; resume <span>→</span></button>}{phase === "interrupted" && !options && <button className="primary-action" onClick={onBack}>Re-enter choices <span>→</span></button>}{phase === "applied" && <button className="primary-action" onClick={() => void verify()}>Verify installation <span>→</span></button>}</div>
    </div>
  );
}

function samePlan(left: InstallPlan, right: InstallPlan) { return JSON.stringify(left) === JSON.stringify(right); }
function phaseFrom(operation: OperationSnapshot | null): Phase { return !operation ? "review" : operation.status === "succeeded" ? "applied" : operation.status === "failed" || operation.status === "interrupted" ? "interrupted" : "applying"; }
function actionLabel(value: string) { return value.split("_").map(word => word[0]?.toUpperCase() + word.slice(1)).join(" "); }
function phaseLabel(phase: Phase) { return phase === "complete" ? "Verified installation" : phase === "review" ? "Mutation preview" : phase === "interrupted" ? "Resumable operation" : "Installation operation"; }
function phaseCopy(phase: Phase, hasOptions: boolean) {
  if (phase === "review") return <><strong>Reviewed, not yet applied.</strong> The host will rebuild this typed plan before mutation.</>;
  if (phase === "applying") return <><strong>Applying on the host.</strong> You may safely reload; progress remains in the private journal.</>;
  if (phase === "interrupted") return <><strong>Operation stopped explicitly.</strong> {hasOptions ? "Revalidate the same plan and resume its idempotent steps." : "Re-enter choices; secrets were not persisted."}</>;
  if (phase === "applied") return <><strong>Changes applied.</strong> Run doctor for the final readiness proof.</>;
  if (phase === "verifying") return <><strong>Running doctor.</strong> Broken embeddings recover according to product policy.</>;
  return <><strong>Installation verified.</strong> Every required doctor check passed.</>;
}
