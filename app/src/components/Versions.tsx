import { useState } from "react";
import type { Command, Comparison, CompareSide, Project, Snapshot } from "../types";

interface VersionsProps {
  project: Project;
  /** Which saved version is being compared, if any. */
  comparing: { snapshot_id: number; side: CompareSide } | null;
  comparison: Comparison | null;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
  onCompare: (snapshotId: number) => void;
  onListen: (side: CompareSide) => void;
  onStopComparing: () => void;
}

/** "−2.3 dB" for a level change, or nothing when there is none. */
function quieter(db: number): string {
  return db < -0.05 ? ` (turned down ${Math.abs(db).toFixed(1)} dB)` : "";
}

/**
 * Saved versions of the song: save one, load one, or compare one with the
 * song as it is now (A/B) at matched loudness.
 */
export default function Versions(props: VersionsProps) {
  const { project } = props;
  const snapshots: Snapshot[] = project.snapshots ?? [];
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [renaming, setRenaming] = useState<number | null>(null);

  const run = (command: Command) => props.onCommand(command).then(props.onEndGesture);
  const save = () => {
    const n = name.trim() || `Version ${snapshots.length + 1}`;
    void run({ command: "take_snapshot", name: n });
    setName("");
  };
  const compared = props.comparing ? snapshots.find((s) => s.id === props.comparing?.snapshot_id) : undefined;

  return (
    <>
      <div className="menu-anchor">
        <button
          className="small"
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          title="Save versions of the song to go back to, or compare them A/B"
        >
          Versions{snapshots.length > 0 ? ` (${snapshots.length})` : ""} ▾
        </button>
        {open && (
          <div className="menu versions-menu" role="menu" aria-label="Versions">
            <form
              className="versions-save"
              onSubmit={(e) => {
                e.preventDefault();
                save();
              }}
            >
              <input
                aria-label="New version name"
                placeholder={`Version ${snapshots.length + 1}`}
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => e.stopPropagation()}
              />
              <button type="submit" className="small" title="Save the song as it is now under this name">
                Save version
              </button>
            </form>
            {snapshots.length === 0 && (
              <p className="muted versions-empty">
                No versions yet. Save one before trying something big; Claude saves one by itself before it starts
                changing your song.
              </p>
            )}
            {[...snapshots].reverse().map((s) => (
              <div className="versions-row" key={s.id}>
                {renaming === s.id ? (
                  <input
                    aria-label="Version name"
                    defaultValue={s.name}
                    autoFocus
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      if (e.key === "Escape") setRenaming(null);
                      if (e.key === "Enter") e.currentTarget.blur();
                    }}
                    onBlur={(e) => {
                      const next = e.target.value.trim();
                      setRenaming(null);
                      if (next && next !== s.name) void run({ command: "rename_snapshot", snapshot_id: s.id, name: next });
                    }}
                  />
                ) : (
                  <span className="versions-name" title="Double-click to rename" onDoubleClick={() => setRenaming(s.id)}>
                    {s.name}
                  </span>
                )}
                <button
                  className="small"
                  onClick={() => {
                    setOpen(false);
                    props.onCompare(s.id);
                  }}
                  title="Switch between this version and the song as it is now while it plays, at the same loudness"
                >
                  A/B
                </button>
                <button
                  className="small"
                  onClick={() => {
                    setOpen(false);
                    void run({ command: "load_snapshot", snapshot_id: s.id });
                  }}
                  title="Replace the song with this version (Ctrl+Z goes back)"
                >
                  Load
                </button>
                <button
                  className="small"
                  aria-label={`Delete version ${s.name}`}
                  onClick={() => void run({ command: "delete_snapshot", snapshot_id: s.id })}
                  title="Delete this version"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {props.comparing && compared && (
        <div className="compare-bar" role="group" aria-label="Compare versions">
          <span className="muted">A/B at matched loudness — press play and switch:</span>
          <button
            className={props.comparing.side === "current" ? "toggle on" : "toggle"}
            aria-pressed={props.comparing.side === "current"}
            onClick={() => props.onListen("current")}
          >
            A: Song now{props.comparison ? quieter(props.comparison.current_gain_db) : ""}
          </button>
          <button
            className={props.comparing.side === "version" ? "toggle on" : "toggle"}
            aria-pressed={props.comparing.side === "version"}
            onClick={() => props.onListen("version")}
          >
            B: {compared.name}
            {props.comparison ? quieter(props.comparison.version_gain_db) : ""}
          </button>
          {snapshots.length > 1 && (
            <select
              aria-label="Compare with another version"
              value={compared.id}
              onChange={(e) => {
                props.onCompare(Number(e.target.value));
                e.currentTarget.blur();
              }}
              title="Switch B to another version (for comparing several options)"
            >
              {snapshots.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          )}
          <button
            className="small"
            onClick={() => {
              props.onStopComparing();
              void run({ command: "load_snapshot", snapshot_id: compared.id });
            }}
            title="Replace the song with this version (Ctrl+Z goes back)"
          >
            Keep B
          </button>
          <button className="small" onClick={props.onStopComparing}>
            Done
          </button>
        </div>
      )}
    </>
  );
}
