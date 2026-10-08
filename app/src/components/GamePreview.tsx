import { useState } from "react";
import type { PreviewPlan } from "../types";

interface GamePreviewProps {
  plan: PreviewPlan;
  /** Song position, to show which section is playing. */
  positionBeats: number;
  /** Where a queued section change happens, if one is waiting. */
  jumpAtBeats: number | null;
  beatsPerBar: number;
  onSwitch: (index: number) => void;
  onLayer: (trackId: number, on: boolean, fadeBeats: number) => void;
  onClose: () => void;
}

const FADES = [
  { label: "At once", beats: 0 },
  { label: "1 beat", beats: 1 },
  { label: "2 beats", beats: 2 },
  { label: "4 beats", beats: 4 },
  { label: "2 bars", beats: 8 },
];

/**
 * Plays the song as the game will: each section loops, a new section
 * starts at the next bar line, and tracks fade in and out like layers.
 */
export default function GamePreview(props: GamePreviewProps) {
  const { plan } = props;
  const [layersOn, setLayersOn] = useState<Record<number, boolean>>({});
  const [fadeBeats, setFadeBeats] = useState(2);
  const [queued, setQueued] = useState<number | null>(null);

  const playing = plan.sections.findIndex(
    (s) => props.positionBeats >= s.start_beats - 1e-6 && props.positionBeats < s.end_beats - 1e-6,
  );
  // The queued change has happened once the playhead is in its section.
  const waiting = queued !== null && queued !== playing ? queued : null;
  const bar = (beats: number) => Math.floor(beats / props.beatsPerBar) + 1;

  return (
    <div className="claude-panel game-preview" role="dialog" aria-label="Game preview">
      <div className="claude-panel-header">
        <strong>Game preview</strong>
        <span className="muted">Hear the song as your game will play the Godot export</span>
        <span className="spacer" />
        <button className="small" onClick={props.onClose} aria-label="Close game preview">
          ✕
        </button>
      </div>

      <section>
        <h3>Sections</h3>
        <p className="muted">
          The playing section loops. Click another one: it starts at the next bar line, like the game&apos;s music
          switching from exploring to combat.
        </p>
        <div className="game-sections" role="group" aria-label="Sections">
          {plan.sections.map((s, i) => (
            <button
              key={`${s.name}-${s.start_beats}`}
              className={i === playing ? "toggle on" : waiting === i ? "toggle queued" : "toggle"}
              aria-pressed={i === playing}
              onClick={() => {
                if (i === playing && waiting === null) return;
                setQueued(i);
                props.onSwitch(i);
              }}
              title={`Bars ${bar(s.start_beats)}–${bar(s.end_beats - 1e-6)}`}
            >
              {s.name}
            </button>
          ))}
        </div>
        {waiting !== null && props.jumpAtBeats !== null && (
          <p className="muted" role="status">
            Switching to {plan.sections[waiting]?.name} at bar {bar(props.jumpAtBeats)}…
          </p>
        )}
      </section>

      <section>
        <h3>Layers</h3>
        <p className="muted">Each track is a layer. Fade them in and out the way your game would, e.g. add drums in combat.</p>
        <label className="field">
          <span>Fade</span>
          <select aria-label="Layer fade length" value={fadeBeats} onChange={(e) => setFadeBeats(Number(e.target.value))}>
            {FADES.map((f) => (
              <option key={f.beats} value={f.beats}>
                {f.label}
              </option>
            ))}
          </select>
        </label>
        <div className="game-layers" role="group" aria-label="Layers">
          {plan.layers.map((l) => {
            const on = layersOn[l.track_id] ?? true;
            return (
              <button
                key={l.track_id}
                className={on ? "toggle on" : "toggle"}
                aria-pressed={on}
                onClick={() => {
                  setLayersOn((s) => ({ ...s, [l.track_id]: !on }));
                  props.onLayer(l.track_id, !on, fadeBeats);
                }}
              >
                {l.name}
              </button>
            );
          })}
        </div>
      </section>
    </div>
  );
}
