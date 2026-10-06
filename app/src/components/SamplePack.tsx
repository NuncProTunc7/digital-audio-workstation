import { useEffect, useState } from "react";
import type { SamplePackStatus, Track } from "../types";

const POLL_MS = 500;

interface SamplePackProps {
  track: Track;
  onChoose: () => void;
  status: (path: string) => Promise<SamplePackStatus>;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** Which sample pack a sampler track plays, and whether it has loaded. */
export default function SamplePack({ track, onChoose, status }: SamplePackProps) {
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
      <button onClick={onChoose}>{path ? "Choose another…" : "Load sample pack…"}</button>
      <p className="muted hint">
        Use any SFZ pack. For a free concert grand, download the Salamander Grand Piano (SFZ + FLAC) from
        freepats.zenvoid.org, unzip it, and choose its .sfz file.
      </p>
    </fieldset>
  );
}
