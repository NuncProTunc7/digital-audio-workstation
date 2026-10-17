import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { FIELDS, silentBecause, type RecipeField } from "../sound";
import type {
  Command,
  Project,
  SoundCategory,
  SoundEffect,
  SoundExportOptions,
  SoundExportReport,
  SoundRecipe,
  Wave,
} from "../types";
import { rememberGodotDir, rememberedGodotDir } from "../godotDir";

interface SoundFxProps {
  project: Project;
  /** Applies a Command; resolves to the project after it (undefined if rejected). */
  onCommand: (command: Command) => Promise<Project | undefined>;
  /** Called when a drag ends, so the next drag is its own undo step. */
  onEndGesture: () => void;
  onPlay: (soundId: number) => void;
  onExport: (options: SoundExportOptions) => Promise<SoundExportReport | undefined>;
  onPickFolder: () => Promise<string | null>;
  onClose: () => void;
  /** Called with the selected sound whenever it changes (null: none). */
  onSelect?: (soundId: number | null) => void;
}

const CATEGORIES: { category: SoundCategory; label: string; title: string }[] = [
  { category: "coin", label: "Coin", title: "A bright pickup ding" },
  { category: "jump", label: "Jump", title: "A springy rising boing" },
  { category: "laser", label: "Laser", title: "A falling zap" },
  { category: "explosion", label: "Explosion", title: "A noisy boom" },
  { category: "hit", label: "Hit", title: "A short thud or smack" },
  { category: "power_up", label: "Power-up", title: "A rising, often repeating, sweep" },
  { category: "blip", label: "Blip", title: "A short menu or UI beep" },
  { category: "random", label: "🎲 Randomize", title: "Anything at all" },
];

const WAVES: { wave: Wave; label: string }[] = [
  { wave: "square", label: "Square (retro)" },
  { wave: "saw", label: "Saw (buzzy)" },
  { wave: "sine", label: "Sine (pure)" },
  { wave: "triangle", label: "Triangle (soft)" },
  { wave: "noise", label: "Noise (hiss)" },
];

/** The simple sliders: one recipe field each, except Length. */
const SIMPLE: { key: RecipeField | "length"; label: string; hint: string }[] = [
  { key: "base_freq", label: "Pitch", hint: "How high it starts" },
  { key: "length", label: "Length", hint: "How long it lasts (hold and fade together)" },
  { key: "freq_ramp", label: "Slide", hint: "Pitch slides down (left) or up (right)" },
  { key: "punch", label: "Punch", hint: "A louder hit at the start" },
  { key: "lpf_freq", label: "Tone", hint: "Duller (left) or brighter (right)" },
  { key: "crush", label: "Crunch", hint: "Lo-fi, bit-crushed grit" },
  { key: "volume_db", label: "Volume", hint: "Overall level in dB" },
];
const SIMPLE_KEYS = new Set<string>(SIMPLE.map((s) => s.key));

/** A random seed, so each button press is a new take. */
function newSeed(): number {
  const a = new Uint32Array(1);
  crypto.getRandomValues(a);
  return a[0];
}

function rangeOf(key: RecipeField | "length"): { min: number; max: number } {
  if (key === "length") return { min: 0, max: 2 };
  const f = FIELDS.find((x) => x.key === key);
  return f ? { min: f.min, max: f.max } : { min: 0, max: 1 };
}

function valueOf(r: SoundRecipe, key: RecipeField | "length"): number {
  return key === "length" ? r.sustain + r.decay : r[key];
}

/**
 * The settings that change when the slider for `key` moves to `v` (only
 * those are sent, so a change your AI made elsewhere isn't undone). Length
 * scales hold and fade together; from nothing, it shares them out as
 * `holdShare` (hold's part of the length before it reached nothing).
 */
function changesFor(r: SoundRecipe, key: RecipeField | "length", v: number, holdShare: number): Partial<SoundRecipe> {
  if (key !== "length") return { [key]: v };
  const total = r.sustain + r.decay;
  if (total <= 0) return { sustain: Math.min(1, v * holdShare), decay: Math.min(1, v * (1 - holdShare)) };
  const scale = v / total;
  return { sustain: Math.min(1, r.sustain * scale), decay: Math.min(1, r.decay * scale) };
}

function format(key: RecipeField | "length", v: number, r: SoundRecipe): string {
  if (key === "volume_db") return `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`;
  if (key === "length") {
    const seconds = ((r.attack ** 2 + r.sustain ** 2 + r.decay ** 2) * 100000) / 44100;
    return `${Math.min(10, seconds).toFixed(2)} s`;
  }
  return v.toFixed(2);
}

/** Game sound effects: make, shape, hear, and export them to Godot. */
export default function SoundFx({
  project,
  onCommand,
  onEndGesture,
  onPlay,
  onExport,
  onPickFolder,
  onClose,
  onSelect,
}: SoundFxProps) {
  const sounds = project.sounds ?? [];
  const [selectedId, setSelectedId] = useState<number | null>(sounds[0]?.id ?? null);
  const selected: SoundEffect | undefined = sounds.find((s) => s.id === selectedId) ?? sounds[0];
  const shownId = selected?.id ?? null;
  useEffect(() => onSelect?.(shownId), [onSelect, shownId]);
  const [more, setMore] = useState(false);
  const [renaming, setRenaming] = useState<number | null>(null);
  const [renameText, setRenameText] = useState("");
  // A drag in progress (so release plays once and ends the undo step).
  const [dragging, setDragging] = useState(false);
  // The drag's last change, so release plays the sound with it in.
  const lastChange = useRef<Promise<unknown>>(Promise.resolve());
  // Each sound's hold share of its length, so Length can come back from 0.
  const holdShares = useRef(new Map<number, number>());
  const [hold, fade] = [selected?.recipe.sustain ?? 0, selected?.recipe.decay ?? 0];
  useEffect(() => {
    if (shownId !== null && hold + fade > 0) holdShares.current.set(shownId, hold / (hold + fade));
  }, [shownId, hold, fade]);
  const [dir, setDir] = useState(rememberedGodotDir);
  const [folder, setFolder] = useState("sfx");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<SoundExportReport | null>(null);

  /** A generated sound of `category`, or (null) a plain blip to shape from scratch. */
  const add = async (category: SoundCategory | null) => {
    const before = new Set(sounds.map((s) => s.id));
    const after = await onCommand(
      category ? { command: "add_sound", category, seed: newSeed() } : { command: "add_sound" },
    );
    onEndGesture();
    const added = after?.sounds?.find((s) => !before.has(s.id));
    if (added) {
      setSelectedId(added.id);
      onPlay(added.id);
    }
  };

  const vary = async () => {
    if (!selected) return;
    const after = await onCommand({ command: "vary_sound", sound_id: selected.id, seed: newSeed() });
    onEndGesture();
    if (after) onPlay(selected.id);
  };

  const setRecipe = (recipe: Partial<SoundRecipe>) => {
    if (!selected) return;
    setDragging(true);
    lastChange.current = onCommand({ command: "set_sound", sound_id: selected.id, recipe });
  };

  const finishDrag = () => {
    if (!dragging || !selected) return;
    setDragging(false);
    const id = selected.id;
    void lastChange.current.then(() => {
      onEndGesture();
      onPlay(id);
    });
  };

  const finishRename = async (sound: SoundEffect) => {
    setRenaming(null);
    const name = renameText.trim();
    if (name && name !== sound.name) {
      await onCommand({ command: "rename_sound", sound_id: sound.id, name });
      onEndGesture();
    }
  };

  const remove = async (sound: SoundEffect) => {
    const index = sounds.findIndex((s) => s.id === sound.id);
    await onCommand({ command: "remove_sound", sound_id: sound.id });
    onEndGesture();
    const next = sounds.filter((s) => s.id !== sound.id);
    setSelectedId(next[Math.min(index, next.length - 1)]?.id ?? null);
  };

  const exportSounds = async (ids: number[] | null) => {
    setBusy(true);
    setReport(null);
    rememberGodotDir(dir);
    const result = await onExport({ project_dir: dir, folder, sound_ids: ids });
    setBusy(false);
    if (result) setReport(result);
  };

  // Space plays the selected sound (rather than the song), except while
  // typing a name or folder.
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.code !== "Space" || !selected) return;
    const el = e.target as HTMLElement;
    if (el.tagName === "TEXTAREA" || (el.tagName === "INPUT" && (el as HTMLInputElement).type === "text")) return;
    e.preventDefault();
    e.stopPropagation();
    onPlay(selected.id);
  };

  // Keys pressed in the panel stay in it (after the slider or field has
  // used them), so Delete, R or note keys don't act on the song behind.
  // Ctrl/Cmd shortcuts (undo, redo, save) still reach the app.
  const keepKeysInPanel = (e: KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey) return;
    e.stopPropagation();
    if (e.key === "Escape") onClose();
  };

  const slider = (key: RecipeField | "length", label: string, hint: string) => {
    if (!selected) return null;
    const r = selected.recipe;
    const { min, max } = rangeOf(key);
    const v = valueOf(r, key);
    return (
      <label className="param" key={key} title={hint}>
        <span className="param-name">{label}</span>
        <input
          type="range"
          min={min}
          max={max}
          step={(max - min) / 200}
          value={v}
          aria-label={label}
          onChange={(e) => setRecipe(changesFor(r, key, Number(e.target.value), holdShares.current.get(selected.id) ?? 0.5))}
          onPointerUp={finishDrag}
          onKeyUp={finishDrag}
          onBlur={finishDrag}
        />
        <span className="param-value">{format(key, v, r)}</span>
      </label>
    );
  };

  return (
    <div className="dialog-backdrop" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog sound-fx" role="dialog" aria-label="Sound effects" onKeyDownCapture={onKeyDown} onKeyDown={keepKeysInPanel}>
        <div className="sound-fx-header">
          <strong>Sound effects</strong>
          <span className="muted">for your game</span>
          <span className="spacer" />
          <button className="small" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>

        <div className="sound-make" role="group" aria-label="Make a new sound">
          <button
            className="small"
            title="A plain short beep with every effect off, to build your own sound from scratch"
            onClick={() => void add(null)}
          >
            + New
          </button>
          {CATEGORIES.map((c) => (
            <button key={c.category} className="small" title={c.title} onClick={() => void add(c.category)}>
              {c.label}
            </button>
          ))}
        </div>

        <div className="sound-body">
          <ul className="sound-list" aria-label="Your sounds">
            {sounds.length === 0 && <li className="muted">No sounds yet. Pick a kind of sound to start.</li>}
            {sounds.map((s) => (
              <li key={s.id} className={s.id === selected?.id ? "selected" : ""}>
                {renaming === s.id ? (
                  <input
                    type="text"
                    aria-label="Sound name"
                    autoFocus
                    value={renameText}
                    onChange={(e) => setRenameText(e.target.value)}
                    onBlur={() => void finishRename(s)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void finishRename(s);
                      if (e.key === "Escape") {
                        // Cancels the rename only, not the whole panel.
                        e.stopPropagation();
                        setRenaming(null);
                      }
                    }}
                  />
                ) : (
                  <button
                    className="sound-name"
                    title="Click to choose, double-click to rename"
                    onClick={() => {
                      setSelectedId(s.id);
                      onPlay(s.id);
                    }}
                    onDoubleClick={() => {
                      setRenaming(s.id);
                      setRenameText(s.name);
                    }}
                  >
                    {s.name}
                  </button>
                )}
              </li>
            ))}
          </ul>

          {selected && (
            <div className="sound-editor">
              <div className="dialog-row">
                <button onClick={() => onPlay(selected.id)} title="Play it (Space)">
                  ▶ Play
                </button>
                <button onClick={() => void vary()} title="A new take on this sound: everything moves a little">
                  ✨ Vary
                </button>
                <span className="spacer" />
                <button className="small" aria-label="Delete sound" title="Delete this sound" onClick={() => void remove(selected)}>
                  Delete
                </button>
              </div>
              {silentBecause(selected.recipe) && (
                <p className="finding warning">
                  <span aria-hidden>⚠</span> {silentBecause(selected.recipe)}.
                </p>
              )}
              <div className="sound-sliders">{SIMPLE.map((s) => slider(s.key, s.label, s.hint))}</div>
              <button className="small sound-more" aria-expanded={more} onClick={() => setMore((m) => !m)}>
                {more ? "Fewer settings ▴" : "More settings ▾"}
              </button>
              {more && (
                <div className="sound-sliders">
                  <label className="param">
                    <span className="param-name">Wave</span>
                    <select
                      aria-label="Wave"
                      value={selected.recipe.wave}
                      onChange={(e) => {
                        void onCommand({
                          command: "set_sound",
                          sound_id: selected.id,
                          recipe: { wave: e.target.value as Wave },
                        }).then((after) => {
                          onEndGesture();
                          if (after) onPlay(selected.id);
                        });
                      }}
                    >
                      {WAVES.map((w) => (
                        <option key={w.wave} value={w.wave}>
                          {w.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  {FIELDS.filter((f) => !SIMPLE_KEYS.has(f.key)).map((f) => slider(f.key, f.label, f.hint))}
                </div>
              )}
            </div>
          )}
        </div>

        <div className="sound-export">
          <strong>Export to Godot</strong>
          <label className="dialog-field">
            <span>Godot project</span>
            <div className="dialog-row">
              <input
                type="text"
                aria-label="Godot project folder"
                placeholder="The folder with project.godot"
                value={dir}
                onChange={(e) => setDir(e.target.value)}
              />
              <button onClick={() => void onPickFolder().then((d) => d && setDir(d))}>Browse…</button>
            </div>
          </label>
          <div className="dialog-row">
            <label className="dialog-field">
              <span>Folder in the project</span>
              <input type="text" aria-label="Folder in the project" value={folder} onChange={(e) => setFolder(e.target.value)} />
            </label>
            <span className="spacer" />
            <button disabled={busy || !dir || !selected} onClick={() => selected && void exportSounds([selected.id])}>
              Export this sound
            </button>
            <button disabled={busy || !dir || sounds.length === 0} onClick={() => void exportSounds(null)}>
              Export all
            </button>
          </div>
          {report && (
            <ul className="sound-report" aria-label="Files written">
              {report.files.map((f) => (
                <li key={f.res_path}>
                  {f.res_path} · {f.seconds.toFixed(2)} s
                  {f.turned_down_db > 0 && <span className="muted"> · turned down {f.turned_down_db.toFixed(1)} dB so it doesn't clip</span>}
                </li>
              ))}
              {report.left_behind.map((f) => (
                <li key={f} className="muted">
                  {f} is still there from before a rename. Delete it in Godot if nothing uses it.
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
