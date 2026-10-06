import { DRUM_BASE_NOTE, labelForNote } from "../keymap";
import type { DrumPad } from "../types";

interface DrumPadsProps {
  pads: DrumPad[];
  activeNotes: ReadonlySet<number>;
  onHit: (note: number) => void;
  onRelease: (note: number) => void;
}

/** 4×4 pad grid, lowest notes at the bottom left like a hardware drum machine. */
export default function DrumPads({ pads, activeNotes, onHit, onRelease }: DrumPadsProps) {
  const rows: DrumPad[][] = [];
  for (let i = 0; i < pads.length; i += 4) rows.unshift(pads.slice(i, i + 4));
  return (
    <div className="drum-pads" role="group" aria-label="Drum pads">
      {rows.flat().map((pad) => {
        const key = labelForNote(pad.note, DRUM_BASE_NOTE);
        return (
          <button
            key={pad.note}
            className={`pad group-${pad.group}${activeNotes.has(pad.note) ? " active" : ""}`}
            onPointerDown={(e) => {
              e.preventDefault();
              onHit(pad.note);
            }}
            onPointerUp={() => onRelease(pad.note)}
            onPointerLeave={() => onRelease(pad.note)}
            title={`${pad.name} (note ${pad.note})`}
          >
            <span className="pad-name">{pad.name}</span>
            {key && <span className="pad-key">{key}</span>}
          </button>
        );
      })}
    </div>
  );
}
