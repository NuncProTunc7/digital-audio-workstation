import { useEffect, useRef } from "react";
import { isBlackKey, labelForNote, noteName } from "../keymap";

interface PianoProps {
  lowNote: number;
  highNote: number;
  activeNotes: ReadonlySet<number>;
  /** First note of the computer-keyboard range, for key labels. */
  baseNote: number;
  onNoteOn: (note: number) => void;
  onNoteOff: (note: number) => void;
}

/** Clickable on-screen keyboard. Drag across keys to glide between notes. */
export default function Piano({ lowNote, highNote, activeNotes, baseNote, onNoteOn, onNoteOff }: PianoProps) {
  const held = useRef<number | null>(null);

  useEffect(() => {
    const release = () => {
      if (held.current !== null) {
        onNoteOff(held.current);
        held.current = null;
      }
    };
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    return () => {
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
    };
  }, [onNoteOff]);

  const press = (note: number) => {
    if (held.current === note) return;
    if (held.current !== null) onNoteOff(held.current);
    held.current = note;
    onNoteOn(note);
  };

  const notes: number[] = [];
  for (let n = lowNote; n <= highNote; n++) notes.push(n);
  const whites = notes.filter((n) => !isBlackKey(n));
  const whiteWidth = 100 / whites.length;

  return (
    <div className="piano" role="group" aria-label="On-screen piano">
      {whites.map((note) => (
        <Key
          key={note}
          note={note}
          black={false}
          style={{ width: `${whiteWidth}%` }}
          active={activeNotes.has(note)}
          label={labelForNote(note, baseNote)}
          onPress={press}
          isDragging={() => held.current !== null}
        />
      ))}
      {notes.filter(isBlackKey).map((note) => {
        const whitesBefore = whites.filter((w) => w < note).length;
        return (
          <Key
            key={note}
            note={note}
            black
            style={{ left: `${whitesBefore * whiteWidth - whiteWidth * 0.32}%`, width: `${whiteWidth * 0.64}%` }}
            active={activeNotes.has(note)}
            label={labelForNote(note, baseNote)}
            onPress={press}
            isDragging={() => held.current !== null}
          />
        );
      })}
    </div>
  );
}

interface KeyProps {
  note: number;
  black: boolean;
  style: React.CSSProperties;
  active: boolean;
  label: string | null;
  onPress: (note: number) => void;
  isDragging: () => boolean;
}

function Key({ note, black, style, active, label, onPress, isDragging }: KeyProps) {
  const classes = ["key", black ? "black" : "white", active ? "active" : "", label ? "in-range" : ""];
  return (
    <div
      className={classes.join(" ")}
      style={style}
      title={noteName(note)}
      data-note={note}
      onPointerDown={(e) => {
        e.preventDefault();
        // Let pointerenter fire on other keys while dragging (touch input
        // captures the pointer by default).
        const el = e.currentTarget;
        if (typeof el.hasPointerCapture === "function" && el.hasPointerCapture(e.pointerId)) {
          el.releasePointerCapture(e.pointerId);
        }
        onPress(note);
      }}
      onPointerEnter={() => {
        if (isDragging()) onPress(note);
      }}
    >
      {label && <span className="key-label">{label}</span>}
      {note % 12 === 0 && <span className="octave-label">{noteName(note)}</span>}
    </div>
  );
}
