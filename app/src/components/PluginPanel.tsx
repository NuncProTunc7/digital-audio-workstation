import { useEffect, useState } from "react";
import type { PluginParam, PluginStatus, Track } from "../types";

const POLL_MS = 500;
/** Parameters shown at once; the search box finds the rest. */
const SHOWN = 60;

interface PluginPanelProps {
  track: Track;
  status: (trackId: number) => Promise<PluginStatus>;
  params: (trackId: number) => Promise<PluginParam[]>;
  onOpenWindow: () => void;
  onParam: (id: number, value: number) => void;
  onEndGesture: () => void;
}

/** A plugin instrument: its window, and its parameters as plain sliders. */
export default function PluginPanel({ track, status, params, onOpenWindow, onParam, onEndGesture }: PluginPanelProps) {
  const plugin = track.instrument.plugin;
  const [state, setState] = useState<PluginStatus | null>(null);
  const [list, setList] = useState<PluginParam[]>([]);
  const [search, setSearch] = useState("");

  // Poll while the plugin starts (big ones take a few seconds).
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const check = () => {
      void status(track.id)
        .then((s) => {
          if (!alive) return;
          setState(s);
          if (s.state === "loading") timer = window.setTimeout(check, POLL_MS);
        })
        .catch(() => {});
    };
    check();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [track.id, plugin?.uid, status]);

  // Values change from the plugin's window, Claude, and undo: re-read them
  // (with the plugin's own wording) whenever the song's copy changes.
  const values = JSON.stringify(plugin?.params ?? {});
  useEffect(() => {
    if (state?.state !== "ready") return;
    let alive = true;
    const timer = window.setTimeout(() => {
      void params(track.id)
        .then((p) => alive && setList(p))
        .catch(() => {});
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [track.id, state?.state, values, params]);

  if (!plugin) return null;
  const needle = search.trim().toLowerCase();
  const matching = list.filter((p) => !needle || p.name.toLowerCase().includes(needle));

  return (
    <div className="plugin-panel">
      <div className="plugin-head">
        <div>
          <strong>{plugin.name}</strong> <span className="muted">{plugin.vendor}</span>
        </div>
        <button onClick={onOpenWindow} disabled={state?.state !== "ready"} title="Show the plugin's own controls">
          Open plugin window
        </button>
      </div>
      {state?.state === "loading" && (
        <p className="muted" role="status">
          Loading the plugin…
        </p>
      )}
      {state?.state === "failed" && (
        <p className="error" role="alert">
          {state.error}. Is it still installed? Choose another plugin, or reinstall it and click Look for new plugins.
        </p>
      )}
      {state?.state === "ready" && list.length > 0 && (
        <fieldset className="param-group plugin-params">
          <legend>Controls</legend>
          {list.length > 8 && (
            <input
              className="plugin-search"
              type="search"
              placeholder={`Search ${list.length} controls`}
              aria-label="Search plugin controls"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => e.stopPropagation()}
            />
          )}
          {matching.slice(0, SHOWN).map((p) => (
            <label key={p.id} className="plugin-param">
              <span className="plugin-param-name">{p.name}</span>
              <input
                type="range"
                min={0}
                max={1}
                step={p.steps > 0 ? 1 / p.steps : 0.001}
                value={plugin.params[String(p.id)] ?? p.value}
                aria-label={p.name}
                onChange={(e) => onParam(p.id, Number(e.target.value))}
                onPointerUp={onEndGesture}
                onKeyUp={onEndGesture}
              />
              <span className="plugin-param-value">
                {p.display}
                {p.units && !p.display.endsWith(p.units) ? ` ${p.units}` : ""}
              </span>
            </label>
          ))}
          {matching.length > SHOWN && (
            <p className="muted hint">{matching.length - SHOWN} more: search to find them.</p>
          )}
        </fieldset>
      )}
    </div>
  );
}
