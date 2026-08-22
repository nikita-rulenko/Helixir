import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  apiPost,
  type AccessCheckResult,
  type AccessProjection,
  type AgentFamilyProjection,
  type AgentProjection,
  type GroupProjection,
  type MutationReceipt,
  type PrincipalProjection,
  type RoleMutation,
} from "../api";
import { StatusDot } from "../components";
import { PageState, relativeAge, useResource } from "./shared";

export type AccessTab = "agents" | "principals" | "groups";
type Notice = { ok: boolean; text: string } | null;
const pageSizes: Record<AccessTab, number> = { agents: 8, principals: 10, groups: 6 };
const groupRoles: RoleMutation["role"][] = ["groupadmin", "moderator", "worker", "viewer"];

export function AccessPage({ initialTab = "groups", initialOnlineOnly = false }: { initialTab?: AccessTab; initialOnlineOnly?: boolean }) {
  const { data, error, loading, refresh } = useResource<AccessProjection>("/access", { pollMs: 15_000 });
  const [tab, setTab] = useState<AccessTab>(initialTab);
  const [query, setQuery] = useState("");
  const [exactPrincipal, setExactPrincipal] = useState<string | null>(null);
  const [onlineOnly, setOnlineOnly] = useState(initialOnlineOnly);
  const [page, setPage] = useState(1);
  const activeFamilies = data?.agent_families.filter(family => family.active).length ?? 0;
  const activeSubagents = data?.agent_families.reduce((sum, family) => sum + family.instances.filter(instance => instance.agent_id !== family.principal_id && instance.active).length, 0) ?? 0;
  const filteredAgents = useMemo(() => data?.agent_families.filter(family => (!onlineOnly || family.active) && searchable(family).includes(query.toLowerCase())) ?? [], [data, query, onlineOnly]);
  const filteredPrincipals = useMemo(() => data?.principals.filter(item => exactPrincipal ? item.subject_id === exactPrincipal : principalText(item).includes(query.toLowerCase())) ?? [], [data, query, exactPrincipal]);
  const filteredGroups = useMemo(() => data?.groups.filter(item => searchable(item).includes(query.toLowerCase())) ?? [], [data, query]);
  const activeRows = tab === "agents" ? filteredAgents : tab === "principals" ? filteredPrincipals : filteredGroups;
  const pageCount = Math.max(1, Math.ceil(activeRows.length / pageSizes[tab]));
  const offset = (Math.min(page, pageCount) - 1) * pageSizes[tab];
  useEffect(() => setPage(1), [tab, query]);

  return <div className="page-canvas section-page access-page">
    <div className="page-heading"><div><p className="eyebrow"><span>03</span> graph-backed administration</p><h1>Access graph</h1><p className="section-lede">The operational console for identities, memberships, roles and shared dedup domains.</p></div><button className="ghost-action" onClick={refresh}>Refresh live state</button></div>
    <section className="mini-metrics" aria-label="Access registry views"><button aria-pressed={tab === "agents"} className={tab === "agents" ? "is-active" : ""} onClick={() => { setTab("agents"); setOnlineOnly(false); setQuery(""); setExactPrincipal(null); setPage(1); }}><span>Logical agents</span><strong>{data?.agent_families.length ?? "—"}</strong><small>{activeFamilies} agents online · {activeSubagents} subagents online</small><i>↗</i></button><button aria-pressed={tab === "principals"} className={tab === "principals" ? "is-active" : ""} onClick={() => { setTab("principals"); setOnlineOnly(false); setQuery(""); setExactPrincipal(null); setPage(1); }}><span>Principals</span><strong>{data?.principals.length ?? "—"}</strong><small>RBAC identities · inspect roles</small><i>↗</i></button><button aria-pressed={tab === "groups"} className={tab === "groups" ? "is-active" : ""} onClick={() => { setTab("groups"); setOnlineOnly(false); setQuery(""); setExactPrincipal(null); setPage(1); }}><span>Groups</span><strong>{data?.groups.length ?? "—"}</strong><small>security domains · administer</small><i>↗</i></button></section>
    <details className="operator-guide"><summary><span>How access works</span><b>Open field guide</b><i>＋</i></summary><div><article><strong>1. Admit</strong><p>New identities begin in <code>onboarding</code>. They remain visible in the registry even after removal.</p></article><article><strong>2. Assign</strong><p>Open a group, add a principal and choose a role. A principal may belong to several groups.</p></article><article><strong>3. Govern</strong><p>Removing a member revokes every active role in that group while keeping the audit history.</p></article><article><strong>4. Federate</strong><p>Dedup federations share visibility for new memories; detaching preserves historical access.</p></article></div></details>
    <PageState loading={loading} error={error} />
    {data && <>
      <section className="contributors-panel"><header><div><p className="eyebrow">Memory stewardship</p><h2>Top contributors</h2></div><span>bounded sample / {data.contributor_sample_size} memories</span></header><div>{data.contributors.map((contributor, index) => <button aria-label={`Show principal ${contributor.user_id}`} key={contributor.user_id} onClick={() => { setTab("principals"); setOnlineOnly(false); setQuery(contributor.user_id); setExactPrincipal(contributor.user_id); setPage(1); }} type="button"><b>{String(index + 1).padStart(2, "0")}</b><strong>{contributor.user_id}</strong><span>{contributor.memories} memories</span><i style={{ width: `${Math.max(8, contributor.memories / Math.max(data.contributors[0]?.memories ?? 1, 1) * 100)}%` }} /></button>)}</div></section>
      <section className="data-panel access-registry">
        <header className="data-toolbar"><div className="segmented">{(["groups", "principals", "agents"] as const).map(item => <button aria-pressed={tab === item} className={tab === item ? "is-active" : ""} onClick={() => { setTab(item); setQuery(""); setExactPrincipal(null); setPage(1); if (item !== "agents") setOnlineOnly(false); }} key={item}>{item}</button>)}</div><div className="toolbar-filters">{tab === "agents" && <button aria-pressed={onlineOnly} className={onlineOnly ? "live-filter is-active" : "live-filter"} onClick={() => setOnlineOnly(value => !value)}><StatusDot ok={onlineOnly} pulse={onlineOnly} />Online only</button>}<label className="search-box"><span>⌕</span><input aria-label="Search access registry" onChange={event => { setQuery(event.target.value); setExactPrincipal(null); setPage(1); }} placeholder="Search id, role, host or group" value={query} /></label></div></header>
        {tab === "groups" && <GroupRegistry data={data} groups={filteredGroups.slice(offset, offset + pageSizes.groups)} onChanged={refresh} />}
        {tab === "principals" && <PrincipalRegistry data={data} principals={filteredPrincipals.slice(offset, offset + pageSizes.principals)} onChanged={refresh} />}
        {tab === "agents" && <AgentRegistry data={data} agents={filteredAgents.slice(offset, offset + pageSizes.agents)} onChanged={refresh} />}
        <Pagination count={activeRows.length} page={Math.min(page, pageCount)} pageCount={pageCount} onPage={setPage} />
      </section>
    </>}
  </div>;
}

function GroupRegistry({ data, groups, onChanged }: { data: AccessProjection; groups: GroupProjection[]; onChanged: () => void }) {
  return <><CreateGroupPanel data={data} onChanged={onChanged} /><DedupFederationPanel data={data} onChanged={onChanged} /><div className="access-accordion group-accordion">{groups.map(group => <GroupRecord data={data} group={group} key={group.group_id} onChanged={onChanged} />)}</div></>;
}

function GroupRecord({ data, group, onChanged }: { data: AccessProjection; group: GroupProjection; onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  const members = useMemo(() => data.principals.filter(principal => principal.groups.some(item => item.group_id === group.group_id)), [data.principals, group.group_id]);
  return <details onToggle={event => setOpen(event.currentTarget.open)}><summary><span className="record-index">{group.reserved ? "RES" : "GRP"}</span><div><strong>{group.name}</strong><small><code>{group.group_id}</code> · {group.description || "Governed memory workspace"}</small></div><div className="summary-badges"><b>{group.group_id === "moirai" ? "admin-only" : `${members.length} members`}</b><em>{group.dedup_group_id ? `dedup / ${group.dedup_group_id}` : "isolated"}</em></div><i>＋</i></summary>{open && <GroupManagement data={data} group={group} members={members} onChanged={onChanged} />}</details>;
}

function GroupManagement({ data, group, members, onChanged }: { data: AccessProjection; group: GroupProjection; members: PrincipalProjection[]; onChanged: () => void }) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const filtered = useMemo(() => members.filter(principal => principalText(principal).includes(query.toLowerCase())), [members, query]);
  const pageSize = 12;
  const pageCount = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const pageMembers = filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize);
  return <div className="group-management"><header><div><p className="eyebrow">{group.group_id === "moirai" ? "Reserved system policy" : "Membership administration"}</p><h3>{group.name}</h3></div>{!group.reserved && <DangerAction label="Deactivate group" body={{ id: group.group_id }} path="/access/groups/deactivate" confirm={`Deactivate group '${group.group_id}'? Role history will remain.`} onChanged={onChanged} />}</header>{group.group_id === "moirai" ? <MoiraiWorkspacePolicy /> : <><AddMemberForm data={data} group={group} onChanged={onChanged} /><div className="member-list"><div className="member-list-toolbar"><label className="search-box"><span>⌕</span><input aria-label={`Search members of ${group.name}`} onChange={event => { setQuery(event.target.value); setPage(1); }} placeholder={`Search ${members.length} members`} value={query} /></label><span>{filtered.length} matching</span></div><div className="member-list-head"><span>Principal</span><span>Active roles</span><span>Action</span></div>{pageMembers.length ? pageMembers.map(principal => <MemberRow group={group} key={principal.subject_id} onChanged={onChanged} principal={principal} />) : <p className="empty-row">{members.length ? "No members match this search." : "No active members. Use the form above to add the first principal."}</p>}<Pagination count={filtered.length} page={currentPage} pageCount={pageCount} onPage={setPage} /></div></>}<DedupAssignment data={data} group={group} onChanged={onChanged} /></div>;
}

function MoiraiWorkspacePolicy() {
  return <section className="reserved-policy"><div><span>∴</span><strong>Membership-free by design</strong></div><p>Moirai hypotheses are generated system memory. No principal can be added to this workspace and no group role is accepted. Global administrators receive read access through the control-plane boundary, not through membership.</p><dl><div><dt>Writers</dt><dd>Clotho · Lachesis · Atropos</dd></div><div><dt>Readers</dt><dd>Global administrators only</dd></div><div><dt>Dedup</dt><dd>Isolated system scope</dd></div></dl></section>;
}

function DedupFederationPanel({ data, onChanged }: { data: AccessProjection; onChanged: () => void }) {
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const mutation = useMutation(onChanged);
  const submit = (event: FormEvent) => { event.preventDefault(); void mutation.run("/access/dedup", { dedup_group_id: id.trim(), name: name.trim(), description: description.trim() }, () => { setId(""); setName(""); setDescription(""); }); };
  return <details className="federation-desk"><summary><div><strong>Dedup federations</strong><span>{data.dedup_groups.length} shared visibility domains</span></div><i>＋</i></summary><div><form onSubmit={submit}><label>Stable ID<input onChange={event => setId(event.target.value)} pattern="[a-z0-9][a-z0-9_-]*" placeholder="engineering" required value={id} /></label><label>Name<input onChange={event => setName(event.target.value)} placeholder="Engineering memory" required value={name} /></label><label>Description<input onChange={event => setDescription(event.target.value)} placeholder="Why these groups share dedup" value={description} /></label><button className="ghost-action" disabled={mutation.busy}>Create federation</button></form><MutationNotice value={mutation.notice} /><div className="federation-list">{data.dedup_groups.map(item => <article key={item.dedup_group_id}><div><strong>{item.name}</strong><code>{item.dedup_group_id}</code><p>{item.description || "Shared future-memory deduplication domain."}</p></div><span>{item.groups.length ? item.groups.join(" · ") : "No groups attached"}</span>{item.groups.length === 0 && <DangerAction label="Deactivate" body={{ id: item.dedup_group_id }} path="/access/dedup/deactivate" confirm={`Deactivate dedup federation '${item.dedup_group_id}'?`} onChanged={onChanged} />}</article>)}</div></div></details>;
}

function CreateGroupPanel({ data, onChanged }: { data: AccessProjection; onChanged: () => void }) {
  const [groupId, setGroupId] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const mutation = useMutation(onChanged);
  const submit = (event: FormEvent) => { event.preventDefault(); void mutation.run("/access/groups", { group_id: groupId.trim(), name: name.trim(), description: description.trim() }, () => { setGroupId(""); setName(""); setDescription(""); }); };
  return <section className="creation-desk"><header><div><p className="eyebrow">Workspace lifecycle</p><h2>Create a memory group</h2></div><span>{data.dedup_groups.length} dedup federations</span></header><form onSubmit={submit}><label>Stable ID<input onChange={event => setGroupId(event.target.value)} pattern="[a-z0-9][a-z0-9_-]*" placeholder="development" required value={groupId} /></label><label>Display name<input onChange={event => setName(event.target.value)} placeholder="Development" required value={name} /></label><label>Description<input onChange={event => setDescription(event.target.value)} placeholder="What this group owns" value={description} /></label><button className="primary-action" disabled={mutation.busy}>Create group</button></form><MutationNotice value={mutation.notice} /></section>;
}

function AddMemberForm({ data, group, onChanged }: { data: AccessProjection; group: GroupProjection; onChanged: () => void }) {
  const [subject, setSubject] = useState("");
  const [role, setRole] = useState<RoleMutation["role"]>("worker");
  const mutation = useMutation(onChanged);
  const submit = (event: FormEvent) => { event.preventDefault(); void mutation.run("/access/groups/add-user", { group_id: group.group_id, subject_id: subject.trim(), role }, () => setSubject("")); };
  return <><form className="member-form" onSubmit={submit}><label>Principal ID<input list={`principals-${group.group_id}`} onChange={event => setSubject(event.target.value)} placeholder="user or agent id" required value={subject} /></label><datalist id={`principals-${group.group_id}`}>{data.principals.map(item => <option key={item.subject_id} value={item.subject_id} />)}</datalist><label>Group role<select onChange={event => setRole(event.target.value as RoleMutation["role"])} value={role}>{groupRoles.map(item => <option key={item} value={item}>{roleLabel(item)}</option>)}</select></label><button className="primary-action" disabled={mutation.busy}>Add to group</button></form><MutationNotice value={mutation.notice} /></>;
}

function MemberRow({ principal, group, onChanged }: { principal: PrincipalProjection; group: GroupProjection; onChanged: () => void }) {
  const roles = principal.groups.find(item => item.group_id === group.group_id)?.roles ?? [];
  return <article><strong>{principal.subject_id}</strong><div className="role-stack">{roles.map(role => <RoleChip group={group.group_id} key={role} subject={principal.subject_id} role={role} onChanged={onChanged} />)}</div><DangerAction label="Remove from group" body={{ group_id: group.group_id, subject_id: principal.subject_id }} path="/access/groups/remove-user" confirm={`Remove '${principal.subject_id}' from '${group.group_id}'?`} onChanged={onChanged} /></article>;
}

function DedupAssignment({ data, group, onChanged }: { data: AccessProjection; group: GroupProjection; onChanged: () => void }) {
  const [federation, setFederation] = useState(data.dedup_groups[0]?.dedup_group_id ?? "");
  const mutation = useMutation(onChanged);
  if (group.reserved) return <p className="reserved-note">Reserved workspaces cannot join dedup federations.</p>;
  return <section className="dedup-desk"><div><strong>Shared dedup domain</strong><p>Attach this group to share new-memory deduplication and visibility with peer groups.</p></div>{group.dedup_group_id ? <button className="ghost-action" disabled={mutation.busy} onClick={() => void mutation.run("/access/dedup/detach", { group_id: group.group_id })}>Detach from {group.dedup_group_id}</button> : <><select aria-label={`Dedup federation for ${group.group_id}`} onChange={event => setFederation(event.target.value)} value={federation}><option value="">Select federation</option>{data.dedup_groups.map(item => <option key={item.dedup_group_id} value={item.dedup_group_id}>{item.name} / {item.dedup_group_id}</option>)}</select><button className="ghost-action" disabled={!federation || mutation.busy} onClick={() => void mutation.run("/access/dedup/attach", { group_id: group.group_id, dedup_group_id: federation })}>Attach</button></>}<MutationNotice value={mutation.notice} /></section>;
}

function PrincipalRegistry({ data, principals, onChanged }: { data: AccessProjection; principals: PrincipalProjection[]; onChanged: () => void }) {
  return <><GlobalAdminForm data={data} onChanged={onChanged} /><PermissionCheckPanel data={data} /><div className="access-accordion">{principals.map(principal => {
    const assignments = principal.global_roles.length + principal.groups.reduce((sum, group) => sum + group.roles.length, 0);
    return <details key={principal.subject_id}><summary><span className="identity-mark">{principal.subject_id.slice(0, 2).toUpperCase()}</span><div><strong>{principal.subject_id}</strong><small>{principal.groups.length} groups · {assignments} active assignments</small></div><div className="summary-badges">{principal.global_roles.map(role => <em className="is-global" key={role}>{role}</em>)}{principal.groups.slice(0, 2).map(group => <b key={group.group_id}>{group.group_id}</b>)}</div><i>＋</i></summary><div className="principal-detail"><h3>Active permissions</h3><div className="role-stack is-left">{principal.global_roles.map(role => <RoleChip key={role} subject={principal.subject_id} role={role} onChanged={onChanged} />)}{principal.groups.flatMap(group => group.roles.map(role => <RoleChip group={group.group_id} key={`${group.group_id}-${role}`} subject={principal.subject_id} role={role} onChanged={onChanged} />))}</div><p>To add this principal to a workspace, open the workspace in the Groups tab. Removing the last group role removes active membership.</p></div></details>;
  })}</div></>;
}

function GlobalAdminForm({ data, onChanged }: { data: AccessProjection; onChanged: () => void }) {
  const [subject, setSubject] = useState("");
  const mutation = useMutation(onChanged);
  const submit = (event: FormEvent) => { event.preventDefault(); void mutation.run("/access/grants", { subject_id: subject.trim(), role: "admin", group_id: null }, () => setSubject("")); };
  return <section className="global-admin-desk"><div><strong>Global administrators</strong><p>Global admin is not a group role. It unlocks the entire graph and this control plane.</p></div><form onSubmit={submit}><input list="global-admin-principals" onChange={event => setSubject(event.target.value)} placeholder="Principal ID" required value={subject} /><datalist id="global-admin-principals">{data.principals.map(item => <option key={item.subject_id} value={item.subject_id} />)}</datalist><button className="ghost-action" disabled={mutation.busy}>Grant admin</button></form><MutationNotice value={mutation.notice} /></section>;
}

function AgentRegistry({ data, agents, onChanged }: { data: AccessProjection; agents: AgentFamilyProjection[]; onChanged: () => void }) {
  return <><section className="agent-presence-contract" aria-label="Agent presence model"><div><span>01</span><p><strong>Agent</strong>One governed principal. It is online when its root process or any child subagent has a live lease.</p></div><div><span>02</span><p><strong>Subagent</strong>A transient delegated worker grouped under its principal. Presence comes from Helixir activity, heartbeat and farewell — never from a host process list.</p></div></section><div className="access-accordion agent-family-accordion">{agents.length === 0 && <p className="agent-family-empty"><strong>No logical agents match this view.</strong><span>Subagent presence is activity-leased. Clear “Online only” or wait for the next Helixir tool call or heartbeat.</span></p>}{agents.map(family => {
    const principal = data.principals.find(item => item.subject_id === family.principal_id);
    const hosts = family.hosts.filter(Boolean);
    const subagents = family.instances.filter(instance => instance.agent_id !== family.principal_id);
    const activeSubagents = subagents.filter(instance => instance.active).length;
    const root = family.instances.find(instance => instance.agent_id === family.principal_id);
    const onlineLabel = activeSubagents > 0 ? `${activeSubagents} ${activeSubagents === 1 ? "SUBAGENT" : "SUBAGENTS"} ONLINE` : root?.active ? "ROOT ONLINE" : "OFFLINE";
    return <details key={family.principal_id}><summary><span className="identity-mark family-mark">{family.principal_id.slice(0, 2).toUpperCase()}</span><div><strong>{family.principal_id}</strong><small>{subagents.length} {subagents.length === 1 ? "subagent" : "subagents"} · {hosts.length ? hosts.join(" · ") : "no host reported"}</small></div><div className={family.active ? "agent-summary-state is-online" : "agent-summary-state"}><StatusDot ok={family.active} pulse={family.active} /><b>{onlineLabel}</b></div><i>＋</i></summary><div className="agent-family-detail"><header><div><p className="eyebrow">Logical agent</p><h3>{family.principal_id}</h3><p>One governed identity with {subagents.length} transient delegated {subagents.length === 1 ? "worker" : "workers"}. Root and child leases are visible separately below.</p></div><dl><div><dt>Subagents online</dt><dd>{activeSubagents} / {subagents.length}</dd></div><div><dt>Hosts</dt><dd>{hosts.length || "—"}</dd></div></dl></header><div className="role-stack is-left family-role-stack">{principal?.global_roles.map(role => <em className="role-chip is-global" key={role}>{role}</em>)}{principal?.groups.flatMap(group => group.roles.map(role => <em className="role-chip" key={`${group.group_id}-${role}`}>{group.group_id} / {role}</em>))}{!principal && <span className="family-unbound">No graph-backed RBAC assignment.</span>}</div><div className="instance-ledger" aria-label={`${family.principal_id} root and subagents`}>{family.instances.map(instance => <AgentInstance isRoot={instance.agent_id === family.principal_id} key={instance.agent_id} instance={instance} onChanged={onChanged} />)}</div></div></details>;
  })}</div></>;
}

function AgentInstance({ instance, isRoot, onChanged }: { instance: AgentProjection; isRoot: boolean; onChanged: () => void }) {
  const terminal = isTerminalStatus(instance.status);
  return <article className={instance.active ? "agent-instance is-online" : "agent-instance"}><div className="instance-identity"><StatusDot ok={instance.active} pulse={instance.active} /><div><span className={isRoot ? "instance-kind is-root" : "instance-kind"}>{isRoot ? "Root agent" : "Subagent"}</span><strong>{instance.name || instance.agent_id}</strong><code>{instance.agent_id}</code></div></div><dl><div><dt>Status</dt><dd>{instance.status || "silent"}</dd></div><div><dt>Host</dt><dd>{instance.host || "unknown"}</dd></div><div><dt>Last signal</dt><dd>{instance.last_seen || "unknown"}<small>{relativeAge(instance.age_seconds)}</small></dd></div><div><dt>Runtime role</dt><dd>{instance.role || "agent"}</dd></div></dl><div className="instance-action">{terminal ? <DangerAction label="Prune instance" body={{ id: instance.agent_id }} path="/access/agents/prune" confirm={`Permanently prune terminal execution instance '${instance.agent_id}' and its presence provenance?`} onChanged={onChanged} /> : <span>{instance.active ? "Lease active" : "Retained for provenance"}</span>}</div></article>;
}

function isTerminalStatus(status: string) {
  return ["done", "failed", "offline", "stopped", "disconnected", "farewell"].includes(status.trim().toLowerCase());
}

function PermissionCheckPanel({ data }: { data: AccessProjection }) {
  const [subject, setSubject] = useState("");
  const [action, setAction] = useState<"read" | "write">("read");
  const [owner, setOwner] = useState("");
  const [result, setResult] = useState<AccessCheckResult | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setBusy(true); setProblem(null); setResult(null);
    try { setResult(await apiPost<AccessCheckResult>("/access/check", { subject_id: subject.trim(), action, owner_id: owner.trim() || null })); }
    catch (reason) { setProblem(reason instanceof Error ? reason.message : "Permission check failed"); }
    finally { setBusy(false); }
  };
  return <details className="permission-simulator"><summary><div><strong>Permission simulator</strong><span>Explain a principal's effective read or write access before changing roles.</span></div><i>＋</i></summary><form onSubmit={submit}><label>Principal<input list="permission-principals" onChange={event => setSubject(event.target.value)} required value={subject} /></label><datalist id="permission-principals">{data.principals.map(item => <option key={item.subject_id} value={item.subject_id} />)}</datalist><label>Action<select onChange={event => setAction(event.target.value as "read" | "write")} value={action}><option value="read">Read</option><option value="write">Write</option></select></label><label>Memory owner<input onChange={event => setOwner(event.target.value)} placeholder="Optional owner ID" value={owner} /></label><button className="ghost-action" disabled={busy}>{busy ? "Checking…" : "Evaluate"}</button></form>{result && <p className={result.allowed ? "permission-result is-allowed" : "permission-result is-denied"}><strong>{result.allowed ? "ALLOWED" : "DENIED"}</strong>{result.explanation}</p>}{problem && <p className="inline-notice">{problem}</p>}</details>;
}

function RoleChip({ subject, role, group, onChanged }: { subject: string; role: string; group?: string; onChanged: () => void }) {
  const mutation = useMutation(onChanged);
  return <em className={group ? "role-chip" : "role-chip is-global"}>{group ? `${group} / ${role}` : role}<button aria-label={`Revoke ${role} from ${subject}`} disabled={mutation.busy} onClick={() => void mutation.run("/access/revocations", { subject_id: subject, role, group_id: group ?? null })} title="Revoke assignment">×</button></em>;
}

function DangerAction({ label, path, body, confirm, onChanged }: { label: string; path: string; body: unknown; confirm: string; onChanged: () => void }) {
  const mutation = useMutation(onChanged);
  return <button className="danger-action" disabled={mutation.busy} onClick={() => { if (window.confirm(confirm)) void mutation.run(path, body); }}>{mutation.busy ? "Working…" : label}</button>;
}

function MutationNotice({ value }: { value: Notice }) { return value ? <p className={value.ok ? "inline-notice is-ok" : "inline-notice"}>{value.text}</p> : null; }

function useMutation(onChanged: () => void) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const run = async (path: string, body: unknown, after?: () => void) => { setBusy(true); setNotice(null); try { const receipt = await apiPost<MutationReceipt>(path, body); setNotice({ ok: true, text: receipt.message }); after?.(); onChanged(); } catch (reason) { setNotice({ ok: false, text: reason instanceof Error ? reason.message : "Mutation failed" }); } finally { setBusy(false); } };
  return { busy, notice, run };
}

function Pagination({ count, page, pageCount, onPage }: { count: number; page: number; pageCount: number; onPage: (page: number) => void }) { return <footer className="pagination"><span>{count} records</span><div><button disabled={page <= 1} onClick={() => onPage(page - 1)}>Previous</button><b>{page} / {pageCount}</b><button disabled={page >= pageCount} onClick={() => onPage(page + 1)}>Next</button></div></footer>; }
function searchable(value: unknown) { return JSON.stringify(value).toLowerCase(); }
function principalText(value: PrincipalProjection) { return `${value.subject_id} ${value.global_roles.join(" ")} ${value.groups.flatMap(group => [group.group_id, ...group.roles]).join(" ")}`.toLowerCase(); }
function roleLabel(value: RoleMutation["role"]) { return ({ groupadmin: "Group admin", moderator: "Moderator", worker: "Worker", viewer: "Viewer", admin: "Global admin" })[value]; }
