import { useState, type FormEvent } from "react";

import { Mark, StatusDot } from "../components";

export function SessionRecovery({ error, onReconnect }: {
  error: string | null;
  onReconnect: (token: string) => Promise<boolean>;
}) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token.trim()) return;
    setBusy(true);
    await onReconnect(token);
    setBusy(false);
  }

  return <main className="session-stage">
    <div className="session-orbit" aria-hidden="true"><i /><i /><i /></div>
    <section className="session-card" aria-labelledby="session-title">
      <div className="session-brand"><Mark /><span>HELIXIR</span></div>
      <p className="eyebrow"><span>ADMIN</span> Control-plane boundary</p>
      <h1 id="session-title">Reconnect to<br /><em>the observatory.</em></h1>
      <p className="session-lede">The dashboard refused an expired credential before reading operational data. Open the authenticated URL printed by <code>helixir web</code>, or paste the stable browser token below.</p>
      <form className="session-form" onSubmit={submit}>
        <label htmlFor="session-token">Browser token</label>
        <div className="session-input-row">
          <input autoComplete="off" autoFocus id="session-token" onChange={event => setToken(event.target.value)} placeholder="64-character private token" spellCheck={false} type="password" value={token} />
          <button className="primary-action" disabled={busy || !token.trim()} type="submit">{busy ? "Verifying…" : "Reconnect"}<span>→</span></button>
        </div>
      </form>
      {error && <div className="session-error" role="alert"><StatusDot ok={false} /><span>{error}</span></div>}
      <div className="session-foot"><span>Fail-closed</span><i />No graph data was exposed</div>
    </section>
  </main>;
}

export function AccessDenied() {
  return <main className="denied-stage"><div className="denied-mark"><Mark /></div><p className="eyebrow"><span>403</span> Graph boundary held</p><h1>Administrators<br /><em>only.</em></h1><p>This control plane is intentionally absent for every group role. Use an authenticated global-admin actor and restart <code>helixir web</code>.</p><div className="denied-rule"><StatusDot ok /><span>HelixDB RBAC denied the session before any dashboard query ran.</span></div></main>;
}
