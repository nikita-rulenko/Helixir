export type BackendState =
  | { kind: "missing" }
  | {
      kind: "managed_local" | "existing_local" | "remote";
      host: string;
      port: number;
      healthy: boolean;
      schema_compatible: boolean;
      container?: string;
      volume?: string;
      image?: string;
    };

export interface DiscoveryResponse {
  phase: "setup" | "ready";
  state: {
    backend: BackendState;
    ollama: { installed: boolean; running: boolean; models: string[] };
    nli_installed: boolean;
    central_config_matches: boolean;
    client_registered: Record<string, boolean>;
    rbac: {
      enabled: boolean;
      migration_active: boolean;
      default_group_exists: boolean;
      onboarding_group_exists: boolean;
      moirai_group_exists: boolean;
      global_admins: string[];
      registered_principals: string[];
    };
  };
}

export interface ControlPlaneMeta {
  product: string;
  version: string;
  api_version: string;
  phase: string;
  transport: string;
  runtime: "control-plane-container" | "native-development";
  host_operations_available: boolean;
}

export interface OverviewStats {
  actor_id: string;
  access_scope: "global" | "groups" | "denied";
  mode: "solo" | "collective" | "collective+insights";
  memories: number | null;
  graph_nodes: number | null;
  principals: number;
  agents: number;
  active_agents: number;
  agent_instances: number;
  active_agent_instances: number;
  subagents: number;
  active_subagents: number;
  workspaces: number;
  entities: number | null;
  concepts: number | null;
}

export interface AgentProjection {
  agent_id: string; name: string; role: string; host: string; status: string;
  last_seen: string; age_seconds: number | null; active: boolean; principal_id: string;
}

export interface AgentFamilyProjection {
  principal_id: string;
  active: boolean;
  instance_count: number;
  active_instances: number;
  hosts: string[];
  instances: AgentProjection[];
}

export interface PrincipalProjection {
  subject_id: string;
  global_roles: string[];
  groups: Array<{ group_id: string; roles: string[] }>;
}

export interface GroupProjection {
  group_id: string; name: string; description: string; dedup_group_id: string | null;
  member_count: number; reserved: boolean;
}

export interface DedupGroupProjection {
  dedup_group_id: string; name: string; description: string; groups: string[];
}

export interface AccessProjection {
  active_window_secs: number;
  agents: AgentProjection[];
  agent_families: AgentFamilyProjection[];
  subagents: AgentProjection[];
  principals: PrincipalProjection[];
  onboarding_principals: PrincipalProjection[];
  groups: GroupProjection[];
  dedup_groups: DedupGroupProjection[];
  contributors: Array<{ user_id: string; memories: number }>;
  contributor_sample_size: number;
}

export interface GatewayConnectionProjection {
  bind: string;
  client_url: string;
  advertised: boolean;
  shareable: boolean;
  auth_enabled: boolean;
  warning: string | null;
}

export interface OnboardingPlacementReport {
  principal_id: string;
  group_id: string;
  group_created: boolean;
  requested_role: string;
  active_roles: string[];
  onboarding_active: boolean;
  onboarding_roles_revoked: string[];
  readable_groups: string[];
  can_write_own_memories: boolean;
  memory_scope: string;
  dedup_group_id?: string;
}

export interface MemoryProjection {
  id: string; internal_id: string; content: string; memory_type: string; user_id: string;
  created_at: string; source: string; rbac_scope: string;
  context_tags: string; groups: string[];
}

export interface MemoryFieldProjection {
  view: "categories" | "memories";
  focus: string | null;
  breadcrumbs: Array<{ id: string; name: string }>;
  categories: Array<{
    id: string; name: string; kind: string; description: string;
    memory_count: number; child_count: number; relation_count: number;
  }>;
  category_edges: Array<{ source: string; target: string; edge_type: string; count: number }>;
  relation_totals: Array<{ edge_type: string; count: number }>;
  memories: MemoryProjection[];
  memory_edges: Array<{ source: string; target: string; edge_type: string }>;
  total_memories: number;
  total_categories: number;
  uncategorized_memories: number;
  page: number;
  page_size: number;
  page_count: number;
  groups: Array<{ group_id: string; name: string }>;
  identities: Array<{ identity: string; kind: "user" | "agent" }>;
  selected_group: string | null;
  selected_identity: string | null;
  query: string | null;
  snapshot_at: string;
  next_refresh_at: string;
}

export interface MoiraiProjection {
  enabled: boolean; mode: string; daemon_active: boolean; daemon_status: string | null;
  insights: Array<{ memory: MemoryProjection; source_groups: string[]; witness_count: number; witnesses: MemoryProjection[]; orphaned: boolean }>;
  stages: Array<{ name: string; responsibility: string; state: string; artifact_count: number; last_activity_at: string | null }>;
  witness_count: number;
  orphan_count: number;
}

export interface MutationReceipt { ok: boolean; message: string }
export interface AccessCheckResult {
  allowed: boolean; subject_id: string; action: "read" | "write";
  owner_id: string | null; explanation: string;
}

export interface RoleMutation {
  subject_id: string;
  role: "admin" | "groupadmin" | "moderator" | "worker" | "viewer";
  group_id: string | null;
}

export interface SystemProjection {
  mode: string; database: string; embedding_provider: string; embedding_model: string;
  llm_provider: string; llm_model: string; nli_required: boolean; rbac_permanent: boolean;
}

export type SchemaKind = "node" | "vector" | "edge";
export type SchemaLifecycle = "active" | "reserved" | "deprecated";

export interface SchemaCensusItem {
  kind: SchemaKind;
  name: string;
  lifecycle: SchemaLifecycle;
  owner: string;
  milestone: string | null;
  producer: string | null;
  consumer: string | null;
  e2e: string | null;
  migration: string | null;
  purpose: string;
  count_key: string;
  count: number | null;
}

export interface SchemaInventoryReport {
  inventory_version: number;
  items: SchemaCensusItem[];
  active: number;
  reserved: number;
  deprecated: number;
  counted: number;
  failed_queries: string[];
}

export interface HealthSnapshot {
  enabled: boolean; container_name: string; memory_used_mib: number | null;
  memory_limit_mib: number | null; memory_percent: number | null; alert_percent: number;
  restart_percent: number; backup_enabled: boolean; newest_backup_age_hours: number | null;
  events: Array<{ at: string; severity: string; kind: string; summary: string; detail: unknown }>;
}

export type HostOperation =
  | { kind: "watch_once" | "watch_stop" | "daemon_stop" | "model_check" }
  | { kind: "watch_start"; interval_secs: number | null }
  | { kind: "daemon_start"; user_id: string; interval_secs: number };

export interface HostOperationResult {
  operation: string; succeeded: boolean; output: string;
}

export interface SettingsSnapshot {
  config_path: string;
  locked_fields: string[];
  mode: "Solo" | "Collective" | "Insights";
  database: { host: string; port: number; instance: string };
  reasoning: { provider: string; model: string; base_url: string; temperature: number; api_key_configured: boolean };
  embeddings: { provider: string; model: string; url: string; api_key_configured: boolean };
  gateway: { bind: string; public_url: string; auth_enabled: boolean };
  swarm: { active_window_secs: number; presence_ttl_secs: number };
  watchdog: {
    enabled: boolean; sample_interval_secs: number; mem_alert_pct: number; mem_restart_pct: number;
    allow_container_restart: boolean; allow_cache_reclaim: boolean; backup_interval_hours: number; backup_keep: number;
  };
}

export interface SettingsPatch {
  mode?: SettingsSnapshot["mode"];
  reasoning_provider?: string; reasoning_model?: string; reasoning_base_url?: string;
  reasoning_temperature?: number; reasoning_api_key?: string;
  embedding_provider?: string; embedding_model?: string; embedding_url?: string; embedding_api_key?: string;
  gateway_public_url?: string;
  swarm_active_window_secs?: number; swarm_presence_ttl_secs?: number;
  watchdog_enabled?: boolean; watchdog_sample_interval_secs?: number;
  watchdog_mem_alert_pct?: number; watchdog_mem_restart_pct?: number;
  watchdog_allow_container_restart?: boolean; watchdog_allow_cache_reclaim?: boolean;
  backup_interval_hours?: number; backup_keep?: number;
}

export interface SettingsMutationReceipt {
  apply: { changed: boolean; config_backup: string | null; reload_required: boolean; settings: SettingsSnapshot };
  reload: { signalled_processes: number; failed_signals: number; restart_required: string[] };
}

export interface BackupRecord { id: string; created_at: string; size_bytes: number; kind: "manual" | "safety" | "automatic" }
export interface BackupInventory {
  available: boolean; reason: string | null; directory: string; retention: number; archives: BackupRecord[];
}
export interface BackupReceipt { operation: string; backup_id: string; safety_backup_id: string | null; message: string }

export interface InstallStep {
  action: { kind: string; configuration?: unknown };
  required: boolean;
  reason: string;
}

export interface InstallPlan {
  steps: InstallStep[];
}

export interface InstallReport {
  steps: Array<{ action: { kind: string; configuration?: unknown }; succeeded: boolean; detail: string | null }>;
  ready: boolean;
  rollback_attempted: boolean;
  rollback_error: string | null;
}

export type OperationStatus = "queued" | "running" | "succeeded" | "failed" | "interrupted";
export interface InstallEvent {
  kind: "plan_started" | "step_started" | "step_succeeded" | "step_failed" | "rollback_started" | "rollback_failed" | "plan_completed";
  step_index: number | null;
  total_steps: number;
  action: { kind: string; configuration?: unknown } | null;
  detail: string | null;
  ready: boolean | null;
}
export interface OperationEvent {
  operation_id: string;
  sequence: number;
  event_id: string;
  step_id: string | null;
  at: string;
  kind: "queued" | "running" | "progress" | "succeeded" | "failed" | "rollback" | "interrupted";
  install: InstallEvent | null;
  detail: string | null;
}
export interface OperationSnapshot {
  operation_id: string;
  plan_fingerprint: string;
  status: OperationStatus;
  created_at: string;
  updated_at: string;
  plan: InstallPlan;
  events: OperationEvent[];
  report: InstallReport | null;
  error: string | null;
  resumable: boolean;
}

export interface DoctorReport {
  ready: boolean;
  checks: Array<{ name: string; status: "pass" | "warn" | "skipped" | "fail"; detail: string; required: boolean }>;
}

export interface InstallOptions {
  mode: "Solo" | "Collective" | "Insights";
  backend:
    | { kind: "provision_local" | "reuse_detected" }
    | { kind: "join_remote"; host: string; port: number };
  local_llm_model: string | null;
  embeddings:
    | { kind: "local_ollama_nomic" }
    | {
        kind: "remote";
        configuration: { provider: string; model: string; url: string; api_key: string };
      };
  clients: Array<"claude_code" | "codex" | "cursor">;
  replace_conflicting_clients: boolean;
  rbac: { operator_id: string; principals: string[] };
}

const tokenKey = "helixir.control-plane-token";
const legacyTokenKey = "helixir.bootstrap-token";
export const sessionExpiredEvent = "helixir:session-expired";

export class ApiError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
  }
}

function controlPlaneToken(): string {
  const hash = new URLSearchParams(window.location.hash.slice(1));
  const incoming = hash.get("token");
  if (incoming) {
    setControlPlaneToken(incoming);
    history.replaceState(null, "", `${location.pathname}${location.search}`);
    return incoming;
  }
  const current = sessionStorage.getItem(tokenKey);
  if (current) return current;
  const legacy = sessionStorage.getItem(legacyTokenKey) ?? "";
  if (legacy) setControlPlaneToken(legacy);
  return legacy;
}

export function setControlPlaneToken(token: string): void {
  sessionStorage.setItem(tokenKey, token.trim());
  sessionStorage.removeItem(legacyTokenKey);
}

function notifyExpiredSession(): void {
  window.dispatchEvent(new Event(sessionExpiredEvent));
}

async function requireOk(response: Response): Promise<Response> {
  if (response.ok) return response;
  if (response.status === 401) notifyExpiredSession();
  const problem = await response.json().catch(() => null) as { message?: string } | null;
  throw new ApiError(
    response.status,
    problem?.message ?? (response.status === 401
      ? "Admin session token is missing or no longer valid"
      : response.status === 403
        ? "Global admin access required"
        : `API ${response.status}`),
  );
}

export async function apiGet<T>(path: string): Promise<T> {
  const response = await fetch(`/api/v1${path}`, {
    headers: { Authorization: `Bearer ${controlPlaneToken()}` },
  });
  return (await requireOk(response)).json() as Promise<T>;
}

export async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`/api/v1${path}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${controlPlaneToken()}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  return (await requireOk(response)).json() as Promise<T>;
}

/** Consume authenticated SSE without placing the session token in a URL. */
export async function apiEventStream(
  path: string,
  after: number,
  onEvent: (event: OperationEvent) => void,
  signal: AbortSignal,
): Promise<number> {
  const separator = path.includes("?") ? "&" : "?";
  const response = await fetch(`/api/v1${path}${separator}after=${after}`, {
    headers: {
      Accept: "text/event-stream",
      Authorization: `Bearer ${controlPlaneToken()}`,
      "Last-Event-ID": String(after),
    },
    signal,
  });
  const body = (await requireOk(response)).body;
  if (!body) throw new Error("Operation stream returned no body");
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let cursor = after;
  while (true) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: !done }).replaceAll("\r\n", "\n");
    let boundary = buffer.indexOf("\n\n");
    while (boundary >= 0) {
      const block = buffer.slice(0, boundary);
      buffer = buffer.slice(boundary + 2);
      const fields = Object.fromEntries(
        block.split("\n").filter(line => !line.startsWith(":"))
          .map(line => [line.slice(0, line.indexOf(":")), line.slice(line.indexOf(":") + 1).trimStart()]),
      );
      if (fields.event === "error") throw new Error(fields.data || "Operation stream failed");
      if (fields.event === "operation" && fields.data) {
        const event = JSON.parse(fields.data) as OperationEvent;
        cursor = Math.max(cursor, event.sequence);
        onEvent(event);
      }
      boundary = buffer.indexOf("\n\n");
    }
    if (done) return cursor;
  }
}
