import { useEffect, useMemo, useState } from "react";

import {
  apiGet, apiPost, type BackupInventory, type BackupReceipt, type SettingsMutationReceipt,
  type SettingsPatch, type SettingsSnapshot,
} from "../api";
import { PageState } from "./shared";

type Draft = {
  mode: SettingsSnapshot["mode"];
  reasoningProvider: string; reasoningModel: string; reasoningUrl: string; reasoningTemperature: string; reasoningKey: string;
  embeddingProvider: string; embeddingModel: string; embeddingUrl: string; embeddingKey: string;
  activeWindow: string; presenceTtl: string; watchdogEnabled: boolean; sampleInterval: string;
  alertPct: string; restartPct: string; allowRestart: boolean; allowReclaim: boolean; backupInterval: string; backupKeep: string;
};

function toDraft(value: SettingsSnapshot): Draft {
  return {
    mode: value.mode,
    reasoningProvider: value.reasoning.provider, reasoningModel: value.reasoning.model,
    reasoningUrl: value.reasoning.base_url, reasoningTemperature: String(value.reasoning.temperature), reasoningKey: "",
    embeddingProvider: value.embeddings.provider, embeddingModel: value.embeddings.model,
    embeddingUrl: value.embeddings.url, embeddingKey: "",
    activeWindow: String(value.swarm.active_window_secs), presenceTtl: String(value.swarm.presence_ttl_secs),
    watchdogEnabled: value.watchdog.enabled, sampleInterval: String(value.watchdog.sample_interval_secs),
    alertPct: String(value.watchdog.mem_alert_pct), restartPct: String(value.watchdog.mem_restart_pct),
    allowRestart: value.watchdog.allow_container_restart, allowReclaim: value.watchdog.allow_cache_reclaim,
    backupInterval: String(value.watchdog.backup_interval_hours), backupKeep: String(value.watchdog.backup_keep),
  };
}

function patchFrom(draft: Draft, original: SettingsSnapshot): SettingsPatch {
  const patch: SettingsPatch = {};
  const set = <K extends keyof SettingsPatch>(key: K, value: SettingsPatch[K], before: unknown) => {
    if (value !== before) patch[key] = value;
  };
  set("mode", draft.mode, original.mode);
  set("reasoning_provider", draft.reasoningProvider, original.reasoning.provider);
  set("reasoning_model", draft.reasoningModel, original.reasoning.model);
  set("reasoning_base_url", draft.reasoningUrl, original.reasoning.base_url);
  set("reasoning_temperature", Number(draft.reasoningTemperature), original.reasoning.temperature);
  if (draft.reasoningKey) patch.reasoning_api_key = draft.reasoningKey;
  set("embedding_provider", draft.embeddingProvider, original.embeddings.provider);
  set("embedding_model", draft.embeddingModel, original.embeddings.model);
  set("embedding_url", draft.embeddingUrl, original.embeddings.url);
  if (draft.embeddingKey) patch.embedding_api_key = draft.embeddingKey;
  set("swarm_active_window_secs", Number(draft.activeWindow), original.swarm.active_window_secs);
  set("swarm_presence_ttl_secs", Number(draft.presenceTtl), original.swarm.presence_ttl_secs);
  set("watchdog_enabled", draft.watchdogEnabled, original.watchdog.enabled);
  set("watchdog_sample_interval_secs", Number(draft.sampleInterval), original.watchdog.sample_interval_secs);
  set("watchdog_mem_alert_pct", Number(draft.alertPct), original.watchdog.mem_alert_pct);
  set("watchdog_mem_restart_pct", Number(draft.restartPct), original.watchdog.mem_restart_pct);
  set("watchdog_allow_container_restart", draft.allowRestart, original.watchdog.allow_container_restart);
  set("watchdog_allow_cache_reclaim", draft.allowReclaim, original.watchdog.allow_cache_reclaim);
  set("backup_interval_hours", Number(draft.backupInterval), original.watchdog.backup_interval_hours);
  set("backup_keep", Number(draft.backupKeep), original.watchdog.backup_keep);
  return patch;
}

const labels: Record<keyof SettingsPatch, string> = {
  mode: "Memory mode", reasoning_provider: "Reasoning provider", reasoning_model: "Reasoning model",
  reasoning_base_url: "Reasoning endpoint", reasoning_temperature: "Reasoning temperature", reasoning_api_key: "Reasoning credential",
  embedding_provider: "Embedding provider", embedding_model: "Embedding model", embedding_url: "Embedding endpoint",
  embedding_api_key: "Embedding credential", swarm_active_window_secs: "Online window", swarm_presence_ttl_secs: "Presence retention",
  watchdog_enabled: "Hygieia policy", watchdog_sample_interval_secs: "Sampling interval", watchdog_mem_alert_pct: "Memory warning",
  watchdog_mem_restart_pct: "Recovery threshold", watchdog_allow_container_restart: "Container recovery",
  watchdog_allow_cache_reclaim: "Cache reclamation", backup_interval_hours: "Backup cadence", backup_keep: "Backup retention",
};

export function SettingsPage({ hostOperationsAvailable }: { hostOperationsAvailable: boolean }) {
  const [settings, setSettings] = useState<SettingsSnapshot | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [vault, setVault] = useState<BackupInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [problem, setProblem] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [restoreId, setRestoreId] = useState<string | null>(null);
  const [confirmation, setConfirmation] = useState("");

  const load = async () => {
    setLoading(true); setProblem(null);
    try {
      const [nextSettings, nextVault] = await Promise.all([
        apiGet<SettingsSnapshot>("/settings"), apiGet<BackupInventory>("/backups"),
      ]);
      setSettings(nextSettings); setDraft(toDraft(nextSettings)); setVault(nextVault);
    } catch (reason) { setProblem(reason instanceof Error ? reason.message : "Administration state unavailable"); }
    finally { setLoading(false); }
  };
  useEffect(() => { void load(); }, []);
  const patch = useMemo(() => settings && draft ? patchFrom(draft, settings) : {}, [draft, settings]);
  const changes = Object.keys(patch) as Array<keyof SettingsPatch>;
  const locked = (field: string) => settings?.locked_fields.includes(field) ?? false;
  const update = <K extends keyof Draft>(key: K, value: Draft[K]) => setDraft(current => current ? { ...current, [key]: value } : current);
  const mutate = async (action: () => Promise<BackupReceipt>, refresh = true) => {
    setBusy(true); setProblem(null); setReceipt(null);
    try { const result = await action(); setReceipt(result.message); if (refresh) setVault(await apiGet<BackupInventory>("/backups")); }
    catch (reason) { setProblem(reason instanceof Error ? reason.message : "Operation failed"); }
    finally { setBusy(false); }
  };
  const apply = async () => {
    setBusy(true); setProblem(null); setReceipt(null);
    try {
      const result = await apiPost<SettingsMutationReceipt>("/settings", patch);
      setSettings(result.apply.settings); setDraft(toDraft(result.apply.settings)); setReviewing(false);
      const restarts = result.reload.restart_required.length ? ` Restart: ${result.reload.restart_required.join(", ")}.` : "";
      setReceipt(`Configuration saved${result.apply.config_backup ? " with an automatic rollback copy" : ""}.${restarts}`);
    } catch (reason) { setProblem(reason instanceof Error ? reason.message : "Settings update failed"); }
    finally { setBusy(false); }
  };

  return <div className="page-canvas section-page settings-page">
    <div className="page-heading"><div><p className="eyebrow"><span>07</span> governed configuration and recovery</p><h1>Stewardship</h1><p className="section-lede">Change the machine deliberately. Secrets stay write-only; every restore begins with a safety snapshot.</p></div><button className="ghost-action" onClick={() => void load()}>Refresh state</button></div>
    <PageState loading={loading} error={!loading && (!settings || !draft || !vault) ? problem : null} />
    {settings && draft && vault && <>
      <section className="settings-ledger"><div><span>Configuration</span><strong>{settings.config_path}</strong></div><div><span>Database</span><strong>{settings.database.host}:{settings.database.port}</strong><small>{settings.database.instance}</small></div><div><span>Host bridge</span><strong className={hostOperationsAvailable ? "is-mint" : "is-warn-text"}>{hostOperationsAvailable ? "AVAILABLE" : "NATIVE MODE"}</strong></div><div><span>Locked by environment</span><strong>{settings.locked_fields.length}</strong></div></section>
      {(problem || receipt) && <p className={receipt ? "mutation-status is-ok" : "mutation-status"} role="status">{receipt ?? problem}</p>}
      <section className="settings-grid">
        <SettingsCard number="01" title="Memory posture" note="Mode changes collective recall and whether the Moirai generative layer runs.">
          <Field label="Mode" locked={locked("mode")}><select disabled={locked("mode")} value={draft.mode} onChange={event => update("mode", event.target.value as Draft["mode"])}><option>Solo</option><option>Collective</option><option>Insights</option></select></Field>
          <Field label="Online window, seconds"><input type="number" value={draft.activeWindow} onChange={event => update("activeWindow", event.target.value)} /></Field>
          <Field label="Presence retention, seconds"><input type="number" value={draft.presenceTtl} onChange={event => update("presenceTtl", event.target.value)} /></Field>
        </SettingsCard>
        <SettingsCard number="02" title="Reasoning circuit" note="Cerebras always resolves to gpt-oss-120b. Replacing a key never returns its prior value.">
          <Field label="Provider" locked={locked("reasoning_provider")}><select disabled={locked("reasoning_provider")} value={draft.reasoningProvider} onChange={event => update("reasoningProvider", event.target.value)}><option value="cerebras">Cerebras</option><option value="deepseek">DeepSeek</option><option value="ollama">Ollama</option></select></Field>
          <Field label="Model" locked={locked("reasoning_model")}><input disabled={locked("reasoning_model")} value={draft.reasoningModel} onChange={event => update("reasoningModel", event.target.value)} /></Field>
          <Field label="Base URL" locked={locked("reasoning_base_url")}><input disabled={locked("reasoning_base_url")} value={draft.reasoningUrl} onChange={event => update("reasoningUrl", event.target.value)} /></Field>
          <Field label="Temperature"><input step="0.05" type="number" value={draft.reasoningTemperature} onChange={event => update("reasoningTemperature", event.target.value)} /></Field>
          <Field label={settings.reasoning.api_key_configured ? "Replace configured API key" : "API key required"} locked={locked("reasoning_api_key")}><input autoComplete="new-password" disabled={locked("reasoning_api_key")} type="password" value={draft.reasoningKey} onChange={event => update("reasoningKey", event.target.value)} placeholder="write-only" /></Field>
        </SettingsCard>
        <SettingsCard number="03" title="Embedding circuit" note="Local Ollama/Nomic is the resilient default; remote OpenAI-compatible embeddings remain explicit.">
          <Field label="Provider" locked={locked("embedding_provider")}><select disabled={locked("embedding_provider")} value={draft.embeddingProvider} onChange={event => update("embeddingProvider", event.target.value)}><option value="ollama">Ollama / Nomic</option><option value="openai">OpenAI compatible</option></select></Field>
          <Field label="Model" locked={locked("embedding_model")}><input disabled={locked("embedding_model")} value={draft.embeddingModel} onChange={event => update("embeddingModel", event.target.value)} /></Field>
          <Field label="Endpoint" locked={locked("embedding_url")}><input disabled={locked("embedding_url")} value={draft.embeddingUrl} onChange={event => update("embeddingUrl", event.target.value)} /></Field>
          <Field label={settings.embeddings.api_key_configured ? "Replace configured API key" : "Remote API key"} locked={locked("embedding_api_key")}><input autoComplete="new-password" disabled={locked("embedding_api_key")} type="password" value={draft.embeddingKey} onChange={event => update("embeddingKey", event.target.value)} placeholder="write-only" /></Field>
        </SettingsCard>
        <SettingsCard number="04" title="Hygieia policy" note="Thresholds are validated together. A zero recovery threshold disables automatic restart.">
          <Toggle label="Enable watchdog" checked={draft.watchdogEnabled} onChange={value => update("watchdogEnabled", value)} />
          <Toggle label="Allow cache reclamation" checked={draft.allowReclaim} onChange={value => update("allowReclaim", value)} />
          <Toggle label="Allow container recovery" checked={draft.allowRestart} onChange={value => update("allowRestart", value)} />
          <Field label="Sample interval, seconds"><input type="number" value={draft.sampleInterval} onChange={event => update("sampleInterval", event.target.value)} /></Field>
          <Field label="Warning / recovery, %"><div className="paired-input"><input type="number" value={draft.alertPct} onChange={event => update("alertPct", event.target.value)} /><input type="number" value={draft.restartPct} onChange={event => update("restartPct", event.target.value)} /></div></Field>
          <Field label="Backup hours / archives"><div className="paired-input"><input type="number" value={draft.backupInterval} onChange={event => update("backupInterval", event.target.value)} /><input type="number" value={draft.backupKeep} onChange={event => update("backupKeep", event.target.value)} /></div></Field>
        </SettingsCard>
      </section>
      <section className="change-dock"><div><p className="eyebrow">Mutation preview</p><strong>{changes.length ? `${changes.length} deliberate change${changes.length === 1 ? "" : "s"}` : "Configuration matches the effective state"}</strong><span>{changes.map(key => labels[key]).join(" · ") || "Nothing is waiting to be written."}</span></div><button className="primary-action compact" disabled={!changes.length || busy} onClick={() => setReviewing(true)}>Review exact change</button></section>
      {reviewing && <section className="review-sheet" role="dialog" aria-modal="true" aria-label="Review configuration changes"><header><div><p className="eyebrow">Exact mutation</p><h2>Review before writing</h2></div><button onClick={() => setReviewing(false)} aria-label="Close review">×</button></header><ol>{changes.map(key => <li key={key}><span>{String(changes.indexOf(key) + 1).padStart(2, "0")}</span><strong>{labels[key]}</strong><code>{key.endsWith("api_key") ? "•••••••• (replacement)" : String(patch[key])}</code></li>)}</ol><p>Helixir validates the complete resulting configuration, writes it atomically, retains the previous file and signals reload-capable processes.</p><button className="primary-action" disabled={busy} onClick={() => void apply()}>{busy ? "Applying…" : "Apply reviewed changes"}</button></section>}
      <section className="vault-panel"><header><div><p className="eyebrow">Managed recovery</p><h2>Backup vault</h2><p>{vault.directory} · retain {vault.retention}</p></div><button className="primary-action compact" disabled={!vault.available || busy} onClick={() => void mutate(() => apiPost("/backups/create", {}))}>Create cold snapshot</button></header>{!vault.available && <p className="inline-notice">{vault.reason}</p>}<div className="vault-list">{vault.archives.length ? vault.archives.map(archive => <article key={archive.id} className={restoreId === archive.id ? "is-open" : ""}><button className="vault-row" onClick={() => { setRestoreId(restoreId === archive.id ? null : archive.id); setConfirmation(""); }}><span className={`archive-kind is-${archive.kind}`}>{archive.kind}</span><strong>{archive.id}</strong><time>{new Date(archive.created_at).toLocaleString()}</time><em>{formatBytes(archive.size_bytes)}</em><i>{restoreId === archive.id ? "−" : "+"}</i></button>{restoreId === archive.id && <div className="restore-drawer"><div><strong>Recovery actions</strong><p>Verification is read-only. Restore stops HelixDB, creates a fresh safety snapshot, restores this archive and waits for the database to return.</p></div><button className="ghost-action" disabled={!vault.available || busy} onClick={() => void mutate(() => apiPost("/backups/verify", { backup_id: archive.id }))}>Verify archive</button><label>Type <code>RESTORE {archive.id}</code><input disabled={!vault.available} value={confirmation} onChange={event => setConfirmation(event.target.value)} /></label><button className="danger-action" disabled={!vault.available || busy || confirmation !== `RESTORE ${archive.id}`} onClick={() => void mutate(() => apiPost("/backups/restore", { backup_id: archive.id, confirmation }))}>Restore with safety copy</button></div>}</article>) : <div className="empty-vault"><span>∅</span><strong>No managed snapshots yet</strong><p>Create the first cold snapshot before the next structural change.</p></div>}</div></section>
    </>}
  </div>;
}

function SettingsCard({ number, title, note, children }: { number: string; title: string; note: string; children: React.ReactNode }) {
  return <section className="settings-card"><header><span>{number}</span><div><h2>{title}</h2><p>{note}</p></div></header><div className="settings-fields">{children}</div></section>;
}
function Field({ label, locked = false, children }: { label: string; locked?: boolean; children: React.ReactNode }) {
  return <label className={locked ? "settings-field is-locked" : "settings-field"}><span>{label}{locked && <em>ENV LOCK</em>}</span>{children}</label>;
}
function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) {
  return <label className="settings-toggle"><span>{label}</span><input checked={checked} onChange={event => onChange(event.target.checked)} type="checkbox" /><i /></label>;
}
function formatBytes(value: number): string {
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(0)} KiB`;
  return `${(value / 1024 / 1024).toFixed(1)} MiB`;
}
