import type { ReactNode } from "react";

export function Mark() {
  return (
    <svg aria-hidden="true" className="h-10 w-10" viewBox="0 0 48 48" fill="none">
      <circle cx="24" cy="24" r="21" stroke="currentColor" strokeOpacity=".35" />
      <path d="M15 13c10 0 17 7 17 11s-7 11-17 11" stroke="#ffb547" strokeWidth="2.4" />
      <path d="M33 13c-10 0-17 7-17 11s7 11 17 11" stroke="#8c7dff" strokeWidth="2.4" />
      <circle cx="15" cy="13" r="2.5" fill="#ffb547" />
      <circle cx="33" cy="35" r="2.5" fill="#8c7dff" />
      <circle cx="24" cy="24" r="2.8" fill="#f4efe6" />
    </svg>
  );
}

export function Glyph({ name }: { name: "overview" | "setup" | "people" | "memory" | "moirai" | "system" | "settings" }) {
  const paths = {
    overview: <path d="M4 13h6V4H4v9Zm10 7h6v-9h-6v9ZM4 20h6v-3H4v3Zm10-13h6V4h-6v3Z" />,
    setup: <path d="m14.5 4-5 5m1-5 5 5M5 14h14M7 18h10" />,
    people: <path d="M16 19v-1.5A3.5 3.5 0 0 0 12.5 14h-5A3.5 3.5 0 0 0 4 17.5V19m9-10a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm3.5 5.2a3.4 3.4 0 0 1 3.5 3.3V19" />,
    memory: <path d="M8 6.5A3.5 3.5 0 0 1 14.5 5 3.5 3.5 0 0 1 18 8.5c0 4.5-6 8.5-6 8.5S6 13 6 8.5A3.5 3.5 0 0 1 8 6.5Zm4 1.5v5m-2.5-2.5h5" />,
    moirai: <path d="M5 6.5h5l2 3 2-3h5M7 17.5h4l1-3 1 3h4M12 9.5v5" />,
    system: <path d="M12 3v3m0 12v3M3 12h3m12 0h3m-3.6-5.4-2.1 2.1M8.7 15.3l-2.1 2.1m10.8 0-2.1-2.1M8.7 8.7 6.6 6.6M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z" />,
    settings: <path d="M5 7h8m4 0h2M5 12h2m4 0h8M5 17h6m4 0h4M13 5v4M7 10v4m6 1v4" />,
  };
  return <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">{paths[name]}</svg>;
}

export function StatusDot({ ok, pulse = false }: { ok: boolean; pulse?: boolean }) {
  return <span className={`status-dot ${ok ? "is-ok" : "is-warn"} ${pulse ? "is-pulsing" : ""}`} />;
}

export function Metric({ eyebrow, value, detail, action, onActivate }: {
  eyebrow: string;
  value: ReactNode;
  detail: string;
  action: string;
  onActivate: () => void;
}) {
  return (
    <button
      aria-label={`${eyebrow}: ${detail}. ${action}`}
      className="metric-card metric-action"
      onClick={onActivate}
      type="button"
    >
      <p className="eyebrow">{eyebrow}</p>
      <div className="metric-value">{value}</div>
      <p className="metric-detail">{detail}</p>
      <span className="metric-cta">{action}<i>↗</i></span>
      <span className="metric-rule" />
    </button>
  );
}

export function MemoryConstellation() {
  return (
    <svg className="constellation" viewBox="0 0 560 360" fill="none" aria-label="Abstract memory graph">
      <defs>
        <linearGradient id="amber" x1="80" y1="20" x2="480" y2="330" gradientUnits="userSpaceOnUse">
          <stop stopColor="#ffc86a" /><stop offset="1" stopColor="#f0783c" />
        </linearGradient>
        <linearGradient id="violet" x1="470" y1="30" x2="150" y2="330" gradientUnits="userSpaceOnUse">
          <stop stopColor="#a99cff" /><stop offset="1" stopColor="#6254dd" />
        </linearGradient>
        <filter id="glow"><feGaussianBlur stdDeviation="5" result="blur" /><feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge></filter>
      </defs>
      <g className="constellation-orbit">
        <ellipse cx="280" cy="180" rx="220" ry="116" stroke="#f4efe6" strokeOpacity=".08" />
        <ellipse cx="280" cy="180" rx="146" ry="210" stroke="#f4efe6" strokeOpacity=".06" transform="rotate(58 280 180)" />
      </g>
      <g className="constellation-lines">
        <path d="M72 212 168 118 278 174 382 80 486 154" stroke="url(#amber)" />
        <path d="M96 94 168 118 238 276 350 238 486 154" stroke="url(#violet)" />
        <path d="m72 212 116 42 50 22 112-38 32-158" stroke="#f4efe6" strokeOpacity=".14" strokeDasharray="4 7" />
      </g>
      <g filter="url(#glow)">
        <circle cx="72" cy="212" r="5" fill="#ffb547" />
        <circle cx="96" cy="94" r="4" fill="#8c7dff" />
        <circle cx="168" cy="118" r="9" fill="#ffbd54" />
        <circle cx="278" cy="174" r="12" fill="#f4efe6" />
        <circle cx="382" cy="80" r="6" fill="#ff9e45" />
        <circle cx="486" cy="154" r="9" fill="#9a8dff" />
        <circle cx="238" cy="276" r="7" fill="#7568f5" />
        <circle cx="350" cy="238" r="6" fill="#ffb547" />
      </g>
    </svg>
  );
}
