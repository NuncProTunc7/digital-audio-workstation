import { useRef } from "react";
import { chordShape, shapeText } from "../guitarChords";
import { noteName } from "../keymap";
import { chordName, guitarStringsFor, scaleOf } from "../music";
import type { ChordQuality, GuitarTuning, Key } from "../types";

/** Time between strings when a chord button strums, low to high. */
const STRUM_MS = 25;

/** Frets with inlay dots; 12 and 24 get two. */
const DOTS = new Set([3, 5, 7, 9, 15, 17, 19, 21]);
const DOUBLE_DOTS = new Set([12, 24]);

/** Name column, the open strings, then frets narrowing up the neck as on a real guitar. */
function fretColumns(frets: number): string {
  const widths = Array.from({ length: frets }, (_, i) => `minmax(18px, ${(0.944 ** i).toFixed(3)}fr)`);
  return `32px 28px ${widths.join(" ")}`;
}

interface FretboardProps {
  /** Open strings without the capo, lowest first. */
  tuning: GuitarTuning;
  capo: number;
  frets: number;
  activeNotes: ReadonlySet<number>;
  songKey?: Key | null;
  /** Chord buttons above the neck. */
  chords?: { root: number; quality: ChordQuality; bass?: number | null }[];
  onPlay: (note: number) => void;
  onRelease: (note: number) => void;
}

/**
 * A guitar neck to play by clicking: high string at the top, as in tab.
 * Frets behind the capo are out of reach. Lit dots show where the notes
 * playing now sit; with a song key, notes in the key are shaded.
 */
export default function Fretboard({ tuning, capo, frets, activeNotes, songKey, chords, onPlay, onRelease }: FretboardProps) {
  // The chord being strummed: its notes, and the strum's pending strings.
  const strum = useRef<{ notes: number[]; timers: number[] } | null>(null);
  const open = tuning.strings.map((n) => n + capo);
  const letGo = () => {
    const s = strum.current;
    if (!s) return;
    s.timers.forEach((t) => window.clearTimeout(t));
    s.notes.forEach(onRelease);
    strum.current = null;
  };
  const strumChord = (notes: number[]) => {
    letGo();
    const played: number[] = [];
    const timers = notes.map((n, i) =>
      window.setTimeout(() => {
        played.push(n);
        onPlay(n);
      }, i * STRUM_MS),
    );
    strum.current = { notes: played, timers };
  };
  const playing = guitarStringsFor(
    [...activeNotes],
    tuning.strings.map((n) => n + capo),
    frets - capo,
  );
  const scale = songKey ? scaleOf(songKey) : null;
  const columns = Array.from({ length: frets + 1 }, (_, f) => f);
  const rows = tuning.strings.map((open, s) => ({ open, s })).reverse();
  return (
    <div className="fretboard" role="group" aria-label="Fretboard">
      {chords && chords.length > 0 && (
        <div className="fretboard-chords" role="group" aria-label="Chords">
          {chords.map((c) => {
            const name = chordName(c);
            const shape = chordShape(c, open, frets - capo);
            if (shape.notes.length === 0) return null;
            return (
              <button
                key={name}
                className="small"
                aria-label={`Play ${name}`}
                title={`${name}: ${shapeText(shape)} (strings low to high; x = not played${capo > 0 ? ", frets from the capo" : ""})`}
                onPointerDown={(e) => {
                  e.preventDefault();
                  strumChord(shape.notes);
                }}
                onPointerUp={letGo}
                onPointerLeave={(e) => {
                  if (e.buttons) letGo();
                }}
              >
                {name}
              </button>
            );
          })}
        </div>
      )}
      <div className="fretboard-grid" style={{ gridTemplateColumns: fretColumns(frets) }}>
        {rows.map(({ open, s }) => (
          <div className="fret-row" role="row" key={s} aria-label={`String ${tuning.strings.length - s}`}>
            <span className="fret-string-name">{noteName(open + capo)}</span>
            {columns.map((f) => {
              const note = open + f;
              const behindCapo = f < capo;
              const lit = !behindCapo && playing.get(note) === s;
              const classes = [
                "fret",
                f === 0 ? "nut" : "",
                f === capo && capo > 0 ? "capo" : "",
                behindCapo ? "behind-capo" : "",
                scale && !behindCapo && scale.has(note % 12) ? "in-key" : "",
                lit ? "active" : "",
              ]
                .filter(Boolean)
                .join(" ");
              return (
                <button
                  key={f}
                  className={classes}
                  disabled={behindCapo}
                  aria-label={`String ${tuning.strings.length - s} fret ${f}: ${noteName(note)}`}
                  aria-pressed={lit}
                  title={`${noteName(note)} (fret ${f})`}
                  onPointerDown={(e) => {
                    e.preventDefault();
                    onPlay(note);
                  }}
                  onPointerUp={() => onRelease(note)}
                  onPointerLeave={(e) => {
                    if (e.buttons) onRelease(note);
                  }}
                >
                  {lit && <span className="fret-note">{noteName(note)}</span>}
                </button>
              );
            })}
          </div>
        ))}
        <div className="fret-row fret-numbers" aria-hidden>
          <span />
          {columns.map((f) => (
            <span key={f} className="fret-number">
              {DOUBLE_DOTS.has(f) ? "••" : DOTS.has(f) ? "•" : ""}
              <small>{f}</small>
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
