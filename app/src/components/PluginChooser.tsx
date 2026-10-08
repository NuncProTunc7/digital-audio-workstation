import { useCallback, useEffect, useState } from "react";
import type { PluginInfo, PluginList } from "../types";

interface PluginChooserProps {
  /** Installed plugins (rescan: look through the plugin folders again). */
  list: (rescan: boolean) => Promise<PluginList>;
  /** The uid of the plugin this track plays, if any. */
  current?: string;
  onChoose: (uid: string) => void;
}

const RESCAN = "__rescan";

/** Picks one of the user's installed VST3 instruments for a track. */
export default function PluginChooser({ list, current, onChoose }: PluginChooserProps) {
  const [plugins, setPlugins] = useState<PluginInfo[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [failed, setFailed] = useState(0);

  const show = useCallback((l: PluginList) => {
    setPlugins(l.plugins.filter((p) => p.kind === "instrument"));
    setFailed(l.could_not_use.length);
  }, []);
  useEffect(() => {
    void list(false)
      .then(show)
      .catch(() => setPlugins([]));
  }, [list, show]);
  const rescan = () => {
    setScanning(true);
    void list(true)
      .then(show)
      .catch(() => setPlugins([]))
      .finally(() => setScanning(false));
  };

  if (plugins === null) return null;
  return (
    <label className="field plugin-chooser">
      <span>Plugin</span>
      <select
        aria-label="Plugin instrument"
        value={current ?? ""}
        disabled={scanning}
        title={
          failed > 0
            ? `${failed} installed plugin(s) couldn't be used; the diagnostic report lists them`
            : "Play this track with one of your installed VST3 instruments"
        }
        onChange={(e) => {
          const v = e.target.value;
          e.currentTarget.blur();
          if (v === RESCAN) rescan();
          else if (v) onChoose(v);
        }}
      >
        <option value="">{scanning ? "Looking for plugins…" : plugins.length ? "Choose a plugin…" : "No plugins found"}</option>
        {plugins.map((p) => (
          <option key={p.uid} value={p.uid}>
            {p.name} ({p.vendor})
          </option>
        ))}
        <option value={RESCAN}>Look for new plugins…</option>
      </select>
    </label>
  );
}
