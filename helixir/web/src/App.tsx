import { useCallback, useEffect, useMemo, useState } from "react";

import {
  apiGet,
  apiPost,
  ApiError,
  sessionExpiredEvent,
  setControlPlaneToken,
  type ControlPlaneMeta,
  type DiscoveryResponse,
  type InstallOptions,
  type InstallPlan,
  type OperationSnapshot,
  type OverviewStats,
} from "./api";
import { Glyph, Mark, MemoryConstellation, Metric, StatusDot } from "./components";
import { AccessPage } from "./pages/AccessPage";
import type { AccessTab } from "./pages/AccessPage";
import { MemoryPage } from "./pages/MemoryPage";
import { MoiraiPage } from "./pages/MoiraiPage";
import { AccessDenied, SessionRecovery } from "./pages/SessionBoundary";
import { activeInstallOperationKey, InstallOperation } from "./pages/InstallOperation";
import { SystemPage } from "./pages/SystemPage";
import { SettingsPage } from "./pages/SettingsPage";

type Section = "overview" | "setup" | "people" | "memory" | "moirai" | "system" | "settings";

const navigation: Array<{ id: Section; label: string }> = [
  { id: "overview", label: "Observatory" },
  { id: "setup", label: "Installation" },
  { id: "people", label: "Access graph" },
  { id: "memory", label: "Memory field" },
  { id: "moirai", label: "Moirai" },
  { id: "system", label: "Hygieia" },
  { id: "settings", label: "Stewardship" },
];

function sectionFromLocation(): Section {
  const candidate = window.location.hash.slice(1);
  return navigation.some(item => item.id === candidate) ? candidate as Section : "overview";
}

function App() {
  const [section, setSection] = useState<Section>(sectionFromLocation);
  const [accessTab, setAccessTab] = useState<AccessTab>("groups");
  const [accessOnlineOnly, setAccessOnlineOnly] = useState(false);
  const [meta, setMeta] = useState<ControlPlaneMeta | null>(null);
  const [discovery, setDiscovery] = useState<DiscoveryResponse | null>(null);
  const [overview, setOverview] = useState<OverviewStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accessDenied, setAccessDenied] = useState(false);
  const [sessionExpired, setSessionExpired] = useState(false);
  const [lastSyncedAt, setLastSyncedAt] = useState<Date | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const navigate = useCallback((next: Section) => {
    if (window.location.hash !== `#${next}`) window.history.pushState(null, "", `#${next}`);
    setSection(next);
  }, []);

  const loadControlPlane = useCallback(async (): Promise<boolean> => {
    setError(null);
    setAccessDenied(false);
    try {
      const nextMeta = await apiGet<ControlPlaneMeta>("/meta");
      setMeta(nextMeta);
      const [nextDiscovery, nextOverview] = await Promise.all([
        nextMeta.host_operations_available
          ? apiGet<DiscoveryResponse>("/discovery")
          : Promise.resolve(null),
        apiGet<OverviewStats>("/overview"),
      ]);
      setDiscovery(nextDiscovery);
      setOverview(nextOverview);
      setLastSyncedAt(new Date());
      setSessionExpired(false);
      return true;
    } catch (reason: unknown) {
      const apiError = reason instanceof ApiError ? reason : null;
      setSessionExpired(apiError?.status === 401);
      setAccessDenied(apiError?.status === 403);
      setError(reason instanceof Error ? reason.message : "Connection failed");
      return false;
    }
  }, []);

  useEffect(() => {
    void loadControlPlane();
  }, [loadControlPlane]);

  useEffect(() => {
    const restore = () => setSection(sectionFromLocation());
    window.addEventListener("popstate", restore);
    window.addEventListener("hashchange", restore);
    return () => {
      window.removeEventListener("popstate", restore);
      window.removeEventListener("hashchange", restore);
    };
  }, []);

  useEffect(() => {
    document.title = `${navigation.find(item => item.id === section)?.label ?? "Helixir"} · Helixir`;
  }, [section]);

  const refreshLive = useCallback(async () => {
    if (refreshing || sessionExpired) return;
    setRefreshing(true);
    try {
      setOverview(await apiGet<OverviewStats>("/overview"));
      setLastSyncedAt(new Date());
      setError(null);
    } catch (reason) {
      const apiError = reason instanceof ApiError ? reason : null;
      if (apiError?.status === 401) setSessionExpired(true);
      else setError(reason instanceof Error ? reason.message : "Live refresh failed");
    } finally {
      setRefreshing(false);
    }
  }, [refreshing, sessionExpired]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshLive();
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [refreshLive]);

  useEffect(() => {
    const expire = () => setSessionExpired(true);
    window.addEventListener(sessionExpiredEvent, expire);
    return () => window.removeEventListener(sessionExpiredEvent, expire);
  }, []);

  const reconnect = useCallback(async (token: string) => {
    setControlPlaneToken(token);
    return loadControlPlane();
  }, [loadControlPlane]);

  const openAccess = useCallback((tab: AccessTab, onlineOnly = false) => {
    setAccessTab(tab);
    setAccessOnlineOnly(onlineOnly);
    navigate("people");
  }, [navigate]);

  const status = useMemo(() => {
    const backend = discovery?.state.backend;
    const backendReady = backend
      ? backend.kind !== "missing" && backend.healthy && backend.schema_compatible
      : overview?.access_scope === "global";
    return {
      ready: discovery?.phase === "ready",
      backend: Boolean(backendReady),
      nli: Boolean(discovery?.state.nli_installed),
      rbac: Boolean(discovery?.state.rbac.enabled),
      ollama: Boolean(discovery?.state.ollama.running),
    };
  }, [discovery, overview]);
  const setupReady = meta !== null
    && overview !== null
    && (!meta.host_operations_available || discovery !== null);

  if (accessDenied) {
    return <AccessDenied />;
  }
  if (sessionExpired) {
    return <SessionRecovery error={error} onReconnect={reconnect} />;
  }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a className="brand" href="#overview" onClick={event => { event.preventDefault(); navigate("overview"); }}>
          <Mark />
          <span><strong>HELIXIR</strong><small>memory observatory</small></span>
        </a>
        <nav aria-label="Primary navigation" className="primary-nav">
          <p className="nav-caption">Control plane / 01</p>
          {navigation.map((item, index) => (
            <button
              aria-label={`${item.label} 0${index + 1}`}
              className={section === item.id ? "nav-item is-active" : "nav-item"}
              key={item.id}
              onClick={() => navigate(item.id)}
              type="button"
            >
              <Glyph name={item.id} />
              <span>{item.label}</span>
              <em>0{index + 1}</em>
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <div className="operator-seal"><span>CX</span><i /></div>
          <div><p>Operator</p><strong>{overview?.actor_id ?? "bootstrap"}</strong></div>
          <button aria-label="Open system anatomy" onClick={() => navigate("system")} type="button">•••</button>
        </div>
      </aside>

      <main className="main-stage">
        <header className="topbar">
          <div className="breadcrumb"><span>HELIXIR</span><i>/</i><strong>{section}</strong></div>
          <div className="topbar-status">
            <button aria-label="Refresh live control-plane data" className="sync-control" disabled={refreshing} onClick={() => void refreshLive()} type="button">
              <span>{refreshing ? "Refreshing" : "Refresh"}</span>
              <i>{lastSyncedAt ? lastSyncedAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "not synced"}</i>
            </button>
            <span className="micro-label">Graph link</span>
            <span className="live-pill"><StatusDot ok={status.backend} pulse />{status.backend ? "Live" : "Awaiting"}</span>
            <span className="version">v{meta?.version ?? "0.17.1-dev"}</span>
          </div>
        </header>

        {section === "overview" ? (
          <Overview discovery={discovery} overview={overview} error={error} status={status} onSetup={() => navigate("setup")} onMemory={() => navigate("memory")} onAccess={openAccess} onSystem={() => navigate("system")} />
        ) : section === "setup" ? (
          setupReady ? (
            <SetupWorkspace
              discovery={discovery}
              actorId={overview.actor_id}
              hostOperationsAvailable={meta.host_operations_available}
              currentMode={overview.mode}
            />
          ) : <SetupLoading problem={error} />
        ) : section === "people" ? <AccessPage initialTab={accessTab} initialOnlineOnly={accessOnlineOnly} />
          : section === "memory" ? <MemoryPage />
          : section === "moirai" ? <MoiraiPage hostOperationsAvailable={meta?.host_operations_available ?? false} />
          : section === "system" ? <SystemPage discovery={discovery} hostOperationsAvailable={meta?.host_operations_available ?? false} />
          : <SettingsPage hostOperationsAvailable={meta?.host_operations_available ?? false} />}
      </main>
    </div>
  );
}

function SetupLoading({ problem }: { problem: string | null }) {
  return (
    <div className="page-canvas section-page">
      <p className="eyebrow"><span>02</span> reversible, visible, deliberate</p>
      <h1>Installation chamber</h1>
      <p className="section-lede">Reading the host before choosing safe installation defaults.</p>
      {problem && <p className="inline-notice" role="alert">{problem}</p>}
    </div>
  );
}

function Overview({ discovery, overview, error, status, onSetup, onMemory, onAccess, onSystem }: {
  discovery: DiscoveryResponse | null;
  overview: OverviewStats | null;
  error: string | null;
  status: Record<string, boolean>;
  onSetup: () => void;
  onMemory: () => void;
  onAccess: (tab: AccessTab, onlineOnly?: boolean) => void;
  onSystem: () => void;
}) {
  const backend = discovery?.state.backend;
  const backendLabel = backend?.kind === "managed_local" ? "Managed local" : backend?.kind === "remote" ? "Remote" : backend?.kind === "existing_local" ? "Existing local" : "Not detected";
  return (
    <div className="page-canvas">
      <section className="hero-panel">
        <div className="hero-copy">
          <p className="eyebrow"><span>01</span> Graph intelligence, under control</p>
          <h1>The elder brain<br /><em>is awake.</em></h1>
          <p className="hero-lede">A living account of provenance, consensus and chains — governed from one quiet room.</p>
          <div className="hero-actions">
            <button className="primary-action" onClick={onSetup} type="button">Inspect installation <span>↗</span></button>
            <button className="ghost-action" onClick={onMemory} type="button">Open memory field</button>
          </div>
        </div>
        <button aria-label="Open live memory graph" className="graph-stage graph-stage-action" onClick={onMemory} type="button">
          <div className="graph-index">GRAPH / LIVE<br /><strong>{backendLabel}</strong></div>
          <MemoryConstellation />
          <div className="graph-caption"><span>PROVENANCE</span><i /> <span>CONSENSUS</span><i /> <span>CHAINS</span></div>
          <span className="graph-stage-cta">Explore live graph <i>↗</i></span>
        </button>
      </section>

      {error && <div className="error-ribbon" role="alert"><strong>Control plane unavailable.</strong> {error}</div>}

      <section className="metrics-grid" aria-label="System overview">
        <Metric eyebrow="Memories" value={overview?.memories?.toLocaleString() ?? "—"} detail={overview?.access_scope === "global" ? "Global bounded count" : "Hidden outside global scope"} action="Explore memory" onActivate={onMemory} />
        <Metric eyebrow="Graph nodes" value={overview?.graph_nodes?.toLocaleString() ?? "—"} detail={`${overview?.entities ?? "—"} entities · ${overview?.concepts ?? "—"} concepts`} action="Inspect graph" onActivate={onMemory} />
        <Metric eyebrow="Agents online" value={`${overview?.active_agents ?? "—"} / ${overview?.agents ?? "—"}`} detail={`${overview?.active_subagents ?? "—"} / ${overview?.subagents ?? "—"} subagents online`} action="Open live roster" onActivate={() => onAccess("agents", true)} />
        <Metric eyebrow="Memory mode" value={<span className="readiness-value mode-value">{overview?.mode ?? "—"}</span>} detail={`${overview?.principals ?? "—"} principals · ${overview?.workspaces ?? "—"} groups`} action="Govern access" onActivate={() => onAccess("groups")} />
      </section>

      <section className="lower-grid">
        <article className="terrain-panel">
          <header><div><p className="eyebrow">Operating posture</p><h2>{overview?.mode ?? "Reading mode…"}</h2></div><button onClick={onMemory} type="button">Open graph ↗</button></header>
          <div className="mode-orbit"><div><span>{overview?.memories?.toLocaleString() ?? "—"}</span><small>governed memories</small></div><i /><i /><i /><p>RBAC remains permanent in every mode. Mode changes collective recall and whether the Moirai generative layer runs.</p></div>
          <footer><span>SOLO</span><span>COLLECTIVE</span><span>INSIGHTS</span></footer>
        </article>

        <article className="systems-panel">
          <header><p className="eyebrow">System anatomy</p><h2>Readiness circuit</h2></header>
          <div className="system-list">
            <SystemRow label="HelixDB" detail={backendLabel} ok={status.backend} />
            <SystemRow label="NLI judge" detail="Contradiction safety" ok={status.nli} />
            <SystemRow label="Graph RBAC" detail="Permanent enforcement" ok={status.rbac} />
            <SystemRow label="Ollama" detail={discovery?.state.ollama.running ? `${discovery.state.ollama.models.length} models online` : "Remote path permitted"} ok={status.ollama} optional />
          </div>
          <button className="panel-link" onClick={onSystem} type="button"><span>Run full diagnostic</span><b>↗</b></button>
        </article>
      </section>
    </div>
  );
}

function SystemRow({ label, detail, ok, optional = false }: { label: string; detail: string; ok: boolean; optional?: boolean }) {
  return <div className="system-row"><StatusDot ok={ok || optional} /><div><strong>{label}</strong><span>{detail}</span></div><em>{ok ? "ONLINE" : optional ? "OPTIONAL" : "ACTION"}</em></div>;
}

function SetupWorkspace({ discovery, actorId, hostOperationsAvailable, currentMode }: {
  discovery: DiscoveryResponse | null;
  actorId: string;
  hostOperationsAvailable: boolean;
  currentMode: OverviewStats["mode"];
}) {
  const detectedClients = Object.keys(discovery?.state.client_registered ?? {}) as InstallOptions["clients"];
  const [clients, setClients] = useState<InstallOptions["clients"]>(detectedClients);
  const [mode, setMode] = useState<InstallOptions["mode"]>(
    currentMode === "solo" ? "Solo" : currentMode.includes("insights") ? "Insights" : "Collective",
  );
  const [backend, setBackend] = useState<"reuse_detected" | "provision_local" | "join_remote">(
    discovery?.state.backend.kind && discovery.state.backend.kind !== "missing" ? "reuse_detected" : "provision_local",
  );
  const [remoteHost, setRemoteHost] = useState("");
  const [remotePort, setRemotePort] = useState("6969");
  const [embeddingStrategy, setEmbeddingStrategy] = useState<"local" | "remote">("local");
  const [embeddingProvider, setEmbeddingProvider] = useState("openai");
  const [embeddingModel, setEmbeddingModel] = useState("");
  const [embeddingUrl, setEmbeddingUrl] = useState("");
  const [embeddingApiKey, setEmbeddingApiKey] = useState("");
  const [localLlmModel, setLocalLlmModel] = useState("");
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [plannedOptions, setPlannedOptions] = useState<InstallOptions | null>(null);
  const [recoveredOperation, setRecoveredOperation] = useState<OperationSnapshot | null>(null);
  const [installStage, setInstallStage] = useState(1);
  const [planning, setPlanning] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => setClients(detectedClients), [discovery]);

  useEffect(() => {
    const operationId = sessionStorage.getItem(activeInstallOperationKey);
    if (!operationId) return;
    void apiGet<OperationSnapshot>(`/install/operations/${operationId}`)
      .then(snapshot => {
        setRecoveredOperation(snapshot);
        setPlan(snapshot.plan);
        setInstallStage(snapshot.status === "succeeded" ? 4 : 3);
      })
      .catch(reason => {
        if (reason instanceof ApiError && reason.status === 404) {
          sessionStorage.removeItem(activeInstallOperationKey);
        } else {
          setProblem("The operation journal is still remembered, but the host supervisor is temporarily unavailable.");
        }
      });
  }, []);

  const review = async () => {
    setPlanning(true);
    setProblem(null);
    const parsedPort = Number(remotePort);
    if (backend === "join_remote" && (!remoteHost.trim() || !Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535)) {
      setProblem("Remote HelixDB requires a host and a port between 1 and 65535.");
      setPlanning(false);
      return;
    }
    if (embeddingStrategy === "remote" && (!embeddingModel.trim() || !embeddingUrl.trim() || !embeddingApiKey.trim())) {
      setProblem("Remote embeddings require a model, API root and API key. The key is sent only to the authenticated host supervisor.");
      setPlanning(false);
      return;
    }
    const options: InstallOptions = {
      mode,
      backend: backend === "join_remote"
        ? { kind: "join_remote", host: remoteHost.trim(), port: parsedPort }
        : { kind: backend },
      local_llm_model: localLlmModel.trim() || null,
      embeddings: embeddingStrategy === "local"
        ? { kind: "local_ollama_nomic" }
        : {
            kind: "remote",
            configuration: {
              provider: embeddingProvider.trim() || "openai",
              model: embeddingModel.trim(),
              url: embeddingUrl.trim(),
              api_key: embeddingApiKey,
            },
          },
      clients,
      replace_conflicting_clients: true,
      rbac: { operator_id: actorId, principals: clients.map(clientPrincipal) },
    };
    try {
      setPlan(await apiPost<InstallPlan>("/install/plan", options));
      setPlannedOptions(options);
      setInstallStage(2);
    } catch (reason) {
      setProblem(reason instanceof Error ? reason.message : "Could not build a safe plan");
    } finally {
      setPlanning(false);
    }
  };
  const stages = ["Discover", "Choose", "Review", "Apply", "Verify"];
  const currentStage = plan ? installStage : 1;
  if (!hostOperationsAvailable) {
    return (
      <div className="page-canvas section-page">
        <p className="eyebrow"><span>02</span> isolated by design</p>
        <h1>Installation chamber</h1>
        <p className="section-lede">The web container cannot inspect or mutate the host directly.</p>
        <div className="section-workbench setup-workbench">
          <ol className="stage-rail">
            {stages.map((stage, index) => <li className={index === 0 ? "is-current" : ""} key={stage}><span>0{index + 1}</span><div><strong>{stage}</strong><small>{index === 0 ? "Supervisor required" : "Protected"}</small></div></li>)}
          </ol>
          <div className="inspection-card choice-card">
            <p className="eyebrow">Host boundary</p>
            <h2>Supervisor not connected</h2>
            <p className="section-lede">A narrow native Helixir supervisor will carry typed installation operations. Docker access and home directories are never mounted into this container.</p>
            <div className="guarantee-line"><StatusDot ok /><span><strong>The dashboard remains live.</strong> Only host discovery and mutations are sealed.</span></div>
            <button className="primary-action" disabled type="button">Awaiting host supervisor <span>→</span></button>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="page-canvas section-page">
      <p className="eyebrow"><span>02</span> reversible, visible, deliberate</p>
      <h1>Installation chamber</h1>
      <p className="section-lede">Choose the shape of this node. Helixir will show every mutation before it touches the machine.</p>
      <div className="section-workbench setup-workbench">
        <ol className="stage-rail">
          {stages.map((stage, index) => <li className={index === currentStage ? "is-current" : index < currentStage ? "is-done" : ""} key={stage}><span>0{index + 1}</span><div><strong>{stage}</strong><small>{index < currentStage ? "Complete" : index === currentStage ? "In focus" : "Waiting"}</small></div></li>)}
        </ol>
        {plan ? <InstallOperation initialOperation={recoveredOperation} options={plannedOptions} plan={plan} onBack={() => { setPlan(null); setPlannedOptions(null); setInstallStage(1); }} onStage={setInstallStage} /> : (
          <div className="inspection-card choice-card">
            <p className="eyebrow">Detected environment</p>
            <h2>{discovery?.state.backend.kind.replaceAll("_", " ") ?? "Reading the host…"}</h2>
            <fieldset>
              <legend>Database ownership</legend>
              <Choice selected={backend === "reuse_detected"} title="Keep this database" detail="Use the reachable HelixDB without taking over its lifecycle." onClick={() => setBackend("reuse_detected")} disabled={discovery?.state.backend.kind === "missing"} />
              <Choice selected={backend === "provision_local"} title="Managed local node" detail="Helixir owns the container, backup and schema lifecycle." onClick={() => setBackend("provision_local")} />
              <Choice selected={backend === "join_remote"} title="Remote HelixDB" detail="Join a separately managed database without changing its lifecycle." onClick={() => setBackend("join_remote")} />
              {backend === "join_remote" && <div className="setup-input-grid"><label>HelixDB host<input autoComplete="off" onChange={event => setRemoteHost(event.target.value)} placeholder="memory.internal" value={remoteHost} /></label><label>Port<input inputMode="numeric" min="1" max="65535" onChange={event => setRemotePort(event.target.value)} value={remotePort} /></label></div>}
            </fieldset>
            <fieldset>
              <legend>Memory mode</legend>
              <div className="client-pills">
                {(["Solo", "Collective", "Insights"] as const).map(value => <button aria-pressed={mode === value} className={mode === value ? "client-pill is-selected" : "client-pill"} key={value} onClick={() => setMode(value)} type="button"><i />{value}</button>)}
              </div>
            </fieldset>
            <fieldset>
              <legend>Embeddings</legend>
              <Choice selected={embeddingStrategy === "local"} title="Ollama + Nomic" detail="Recommended local embeddings. Doctor installs and repairs this path automatically." onClick={() => setEmbeddingStrategy("local")} />
              <Choice selected={embeddingStrategy === "remote"} title="Remote embedding API" detail="Use an explicitly configured OpenAI-compatible embedding service." onClick={() => setEmbeddingStrategy("remote")} />
              {embeddingStrategy === "remote" && <div className="setup-input-grid is-remote"><label>Provider<input onChange={event => setEmbeddingProvider(event.target.value)} value={embeddingProvider} /></label><label>Model<input onChange={event => setEmbeddingModel(event.target.value)} placeholder="text-embedding-3-small" value={embeddingModel} /></label><label>API root<input onChange={event => setEmbeddingUrl(event.target.value)} placeholder="https://api.example.com/v1" type="url" value={embeddingUrl} /></label><label>API key<input autoComplete="new-password" onChange={event => setEmbeddingApiKey(event.target.value)} type="password" value={embeddingApiKey} /></label></div>}
            </fieldset>
            <fieldset>
              <legend>Optional local reasoning fallback</legend>
              <label className="setup-wide-input">Ollama model<input onChange={event => setLocalLlmModel(event.target.value)} placeholder="Leave empty to use only the configured remote reasoning LLM" value={localLlmModel} /></label>
              <p className="field-note">No model is selected silently. If you need a local fallback, enter an explicit model such as <code>gpt-oss:20b</code>.</p>
            </fieldset>
            <fieldset>
              <legend>Agent clients</legend>
              <div className="client-pills">
                {detectedClients.map(client => <button aria-pressed={clients.includes(client)} className={clients.includes(client) ? "client-pill is-selected" : "client-pill"} key={client} onClick={() => setClients(toggleClient(clients, client))} type="button"><i />{client.replace("_", " ")}</button>)}
              </div>
            </fieldset>
            {clients.length > 0 && <p className="registration-consent"><strong>Reviewed replacement consent.</strong> Apply backs up and replaces only conflicting <code>helixir-local</code> entries for the selected clients; unrelated configuration is preserved.</p>}
            <div className="guarantee-line"><StatusDot ok /><span><strong>Local NLI is always required.</strong> Embeddings use {embeddingStrategy === "local" ? "Ollama + Nomic with doctor recovery" : "the explicit remote service above"}.</span></div>
            {problem && <p className="form-problem" role="alert">{problem}</p>}
            <button className="primary-action" disabled={planning} onClick={review} type="button">{planning ? "Composing plan…" : "Review exact plan"}<span>→</span></button>
          </div>
        )}
      </div>
    </div>
  );
}

function Choice({ selected, title, detail, onClick, disabled = false }: { selected: boolean; title: string; detail: string; onClick: () => void; disabled?: boolean }) {
  return <button aria-pressed={selected} className={selected ? "choice-row is-selected" : "choice-row"} disabled={disabled} onClick={onClick} type="button"><i /><span><strong>{title}</strong><small>{detail}</small></span><em>{selected ? "SELECTED" : "CHOOSE"}</em></button>;
}

function toggleClient(clients: InstallOptions["clients"], client: InstallOptions["clients"][number]) {
  return clients.includes(client) ? clients.filter(item => item !== client) : [...clients, client];
}

function clientPrincipal(client: InstallOptions["clients"][number]) {
  return client === "claude_code" ? "claude" : client;
}

export default App;
