import { useState, type FormEvent } from "react";

import {
  apiPost,
  type AccessProjection,
  type GatewayConnectionProjection,
  type OnboardingPlacementReport,
  type PrincipalProjection,
  type RoleMutation,
} from "../api";
import { StatusDot } from "../components";

const placementRoles: RoleMutation["role"][] = ["worker", "viewer", "moderator", "groupadmin"];

export function GatewayHandoff({ data, error, loading }: { data: GatewayConnectionProjection | null; error: string | null; loading: boolean }) {
  const [copied, setCopied] = useState<"url" | "command" | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);
  const copy = async (kind: "url" | "command", value: string) => {
    setCopyError(null);
    try {
      await copyText(value);
      setCopied(kind);
      window.setTimeout(() => setCopied(current => current === kind ? null : current), 1800);
    } catch {
      setCopyError("Clipboard access was blocked. Select and copy the endpoint manually.");
    }
  };
  const command = data ? `helixir-client connect --gateway "${data.client_url}"` : "";
  return <section className={data?.shareable ? "gateway-handoff is-ready" : "gateway-handoff"}>
    <div className="gateway-signal"><span>↗</span><div><p className="eyebrow">Remote client handoff</p><h2>{loading ? "Reading gateway…" : data?.shareable ? "Connection coordinates ready" : "Gateway address needs attention"}</h2><p>Give an agent host this MCP endpoint. HelixDB stays private behind Helixir.</p></div></div>
    {data && <div className="gateway-coordinate"><span>Client endpoint</span><code>{data.client_url}</code><small>configured bind {data.bind} · {data.auth_enabled ? "bearer token required" : "trusted network"}</small></div>}
    {data && <div className="gateway-actions"><button className="primary-action compact" onClick={() => void copy("url", data.client_url)}>{copied === "url" ? "Copied" : "Copy endpoint"}</button><button className="ghost-action" onClick={() => void copy("command", command)}>{copied === "command" ? "Copied" : "Copy connect command"}</button></div>}
    {(copyError || error || data?.warning) && <p className="gateway-warning" role="status">{copyError ?? error ?? data?.warning}</p>}
  </section>;
}

async function copyText(value: string) {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return;
    } catch {
      // Some browsers expose the API but deny it outside a trusted gesture.
    }
  }
  const field = document.createElement("textarea");
  field.value = value;
  field.setAttribute("readonly", "");
  field.style.position = "fixed";
  field.style.opacity = "0";
  document.body.append(field);
  field.select();
  const copied = document.execCommand?.("copy") ?? false;
  field.remove();
  if (!copied) throw new Error("clipboard unavailable");
}

export function OnboardingRegistry({ data, principals, onChanged, onOpenGroups }: { data: AccessProjection; principals: PrincipalProjection[]; onChanged: () => void; onOpenGroups: () => void }) {
  const groups = data.groups.filter(group => !group.reserved);
  const [receipt, setReceipt] = useState<OnboardingPlacementReport | null>(null);
  return <div className="onboarding-inbox">
    <header><div><p className="eyebrow">Admission queue</p><h2>New principals waiting for placement</h2><p>Each identity below has a temporary worker grant in <code>onboarding</code>. Assigning it runs one resumable server-side transition and removes that temporary grant only after the working role is verified.</p></div><span>{principals.length} pending</span></header>
    {receipt && <p className="placement-receipt" role="status"><strong>{receipt.principal_id}</strong> joined <strong>{receipt.group_id}</strong> as {receipt.requested_role}. Effective scope: <code>{receipt.memory_scope}</code>.</p>}
    {!groups.length && <section className="onboarding-empty is-blocked"><span>01</span><div><strong>Create the first working group</strong><p>Reserved workspaces cannot be used as a normal visibility boundary.</p></div><button className="primary-action compact" onClick={onOpenGroups}>Open group administration</button></section>}
    {groups.length > 0 && principals.length === 0 && <section className="onboarding-empty"><span>✓</span><div><strong>Admission queue is clear</strong><p>New remote clients appear here immediately after <code>helixir-client connect</code> enrolls their stable principal.</p></div></section>}
    <div className="onboarding-list">{principals.map(principal => <OnboardingRow data={data} groups={groups} key={principal.subject_id} onPlaced={report => { setReceipt(report); onChanged(); }} principal={principal} />)}</div>
  </div>;
}

function OnboardingRow({ data, groups, principal, onPlaced }: { data: AccessProjection; groups: AccessProjection["groups"]; principal: PrincipalProjection; onPlaced: (report: OnboardingPlacementReport) => void }) {
  const [groupId, setGroupId] = useState(groups[0]?.group_id ?? "");
  const [role, setRole] = useState<RoleMutation["role"]>("worker");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const family = data.agent_families.find(item => item.principal_id === principal.subject_id);
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setBusy(true); setProblem(null);
    try {
      const report = await apiPost<OnboardingPlacementReport>("/access/onboarding/assign", { principal_id: principal.subject_id, group_id: groupId, role });
      onPlaced(report);
    } catch (reason) { setProblem(reason instanceof Error ? reason.message : "Placement failed"); }
    finally { setBusy(false); }
  };
  return <article><div className="onboarding-identity"><span>{principal.subject_id.slice(0, 2).toUpperCase()}</span><div><strong>{principal.subject_id}</strong><small><StatusDot ok={family?.active ?? false} pulse={family?.active ?? false} />{family?.active ? `${family.active_instances} live instance${family.active_instances === 1 ? "" : "s"}` : family ? "registered · currently offline" : "registered · no presence yet"}</small></div></div><form onSubmit={submit}><label>Visibility group<select disabled={!groups.length || busy} onChange={event => setGroupId(event.target.value)} required value={groupId}><option value="">Choose group</option>{groups.map(group => <option key={group.group_id} value={group.group_id}>{group.name} / {group.group_id}</option>)}</select></label><label>Role<select disabled={busy} onChange={event => setRole(event.target.value as RoleMutation["role"])} value={role}>{placementRoles.map(item => <option key={item} value={item}>{roleLabel(item)}</option>)}</select></label><button className="primary-action compact" disabled={!groupId || busy}>{busy ? "Assigning…" : "Assign & admit"}</button></form>{problem && <p className="inline-notice">{problem}</p>}</article>;
}

function roleLabel(value: RoleMutation["role"]) {
  return ({ groupadmin: "Group admin", moderator: "Moderator", worker: "Worker", viewer: "Viewer", admin: "Global admin" })[value];
}
