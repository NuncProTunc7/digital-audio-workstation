import { useCallback, useEffect, useState } from "react";
import type { LibraryPack, SamplePackStatus, Track } from "../types";

const POLL_MS = 500;

interface SamplePackProps {
  track: Track;
  onChoose: () => void;
  status: (path: string) => Promise<SamplePackStatus>;
  /** The free instrument library; omitted where downloads aren't possible. */
  library?: () => Promise<LibraryPack[]>;
  onDownload?: (id: string) => Promise<void>;
  /** Load a library instrument's program (.sfz path) on this track. */
  onUse?: (path: string) => void;
}

function jobText(pack: LibraryPack): string | null {
  const job = pack.job;
  if (!job) return null;
  if (job.state === "unpacking") return "Unpacking…";
  if (job.state === "failed") return null;
  const total = job.total ?? pack.megabytes * 1_000_000;
  return `Downloading… ${Math.min(99, Math.round((job.bytes / total) * 100))}%`;
}

/** Free instruments to download and use, from their publishers. */
function Library({
  library,
  onDownload,
  onUse,
}: {
  library: () => Promise<LibraryPack[]>;
  onDownload: (id: string) => Promise<void>;
  onUse: (path: string) => void;
}) {
  const [packs, setPacks] = useState<LibraryPack[] | null>(null);
  const refresh = useCallback(() => {
    void library()
      .then(setPacks)
      .catch(() => {});
  }, [library]);
  useEffect(refresh, [refresh]);
  // Watch downloads (Claude may have started one too).
  const busy = packs?.some((p) => p.job && p.job.state !== "failed") ?? false;
  useEffect(() => {
    if (!busy) return;
    const timer = window.setInterval(refresh, 1000);
    return () => window.clearInterval(timer);
  }, [busy, refresh]);

  if (!packs) return null;
  return (
    <details className="library">
      <summary>Free instruments: pianos, strings, brass, woodwinds</summary>
      <ul>
        {packs.map((p) => {
          const progress = jobText(p);
          return (
            <li key={p.id}>
              <div>
                <strong>{p.name}</strong> <span className="muted">{p.description}</span>
              </div>
              {p.installed ? (
                <div className="library-programs">
                  {p.installed.map(([name, path]) => (
                    <button key={path} onClick={() => onUse(path)} title={`Play ${name} on this track`}>
                      Use {name}
                    </button>
                  ))}
                </div>
              ) : progress ? (
                <div className="muted" role="status">
                  {progress}
                </div>
              ) : (
                <div className="library-programs">
                  <button
                    onClick={() => void onDownload(p.id).then(refresh)}
                    title="Download it from its publisher (it stays on this computer for every song)"
                  >
                    {p.job?.state === "failed" ? "Try again" : "Download"} ({p.megabytes} MB)
                  </button>
                  {p.job?.state === "failed" && (
                    <span className="error" role="alert">
                      {p.job.error}
                    </span>
                  )}
                </div>
              )}
              <div className="muted hint">
                {p.license.startsWith("CC0") ? "Free to use, no credit needed." : `Credit in your game: ${p.credit}`}
              </div>
            </li>
          );
        })}
      </ul>
    </details>
  );
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** Which sample pack a sampler track plays, and whether it has loaded. */
export default function SamplePack({ track, onChoose, status, library, onDownload, onUse }: SamplePackProps) {
  const path = track.instrument.sample_pack ?? null;
  const [state, setState] = useState<SamplePackStatus | null>(null);

  // Poll while loading; a big piano takes a few seconds.
  useEffect(() => {
    if (!path) return;
    let alive = true;
    let timer: number | undefined;
    const check = () => {
      void status(path)
        .then((s) => {
          if (!alive) return;
          setState(s);
          if (s.state === "loading" || s.state === "not_loaded") timer = window.setTimeout(check, POLL_MS);
        })
        .catch(() => {});
    };
    check();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [path, status]);

  return (
    <fieldset className="param-group sample-pack">
      <legend>Sample pack</legend>
      {path ? (
        <p title={path}>
          <strong>{fileName(path)}</strong>{" "}
          {state?.state === "loading" || state?.state === "not_loaded" ? (
            <span className="muted">loading…</span>
          ) : state?.state === "ready" ? (
            <span className="muted">
              {state.zones} samples, {Math.round(state.megabytes ?? 0)} MB
              {state.layers_kept !== undefined && state.layers_total !== undefined && state.layers_kept < state.layers_total
                ? ` (${state.layers_kept} of ${state.layers_total} velocity layers, to save memory)`
                : ""}
            </span>
          ) : state?.state === "failed" ? (
            <span className="error" role="alert">
              {state.error}
            </span>
          ) : null}
        </p>
      ) : (
        <p className="muted">No sample pack loaded yet: this track is silent.</p>
      )}
      {library && onDownload && onUse && <Library library={library} onDownload={onDownload} onUse={onUse} />}
      <button onClick={onChoose}>{path ? "Choose another file…" : "Load sample pack file…"}</button>
      <p className="muted hint">Or use any SFZ pack you have: unzip it and choose its .sfz file.</p>
    </fieldset>
  );
}
