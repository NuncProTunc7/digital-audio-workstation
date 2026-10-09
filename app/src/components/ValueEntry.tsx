import { useState } from "react";
import type { KeyboardEvent } from "react";

interface ValueEntryProps {
  /** What the readout shows, such as "2.1 kHz". */
  text: string;
  /** Name of the setting, for screen readers ("Cutoff", "Keys volume"). */
  name: string;
  /** Reads typed text; null when it isn't understood. */
  parse: (text: string) => number | null;
  min: number;
  max: number;
  /** The allowed range in the readout's units, such as "-60 to +6 dB". */
  rangeText: string;
  /** Sets the value (then the caller ends the undo step). */
  onSet: (value: number) => void;
  className?: string;
}

/**
 * A value readout you can type into: double-click it (or focus it and press
 * Enter), type a value with or without its unit, and press Enter. Escape
 * cancels. Text that can't be read, or is out of range, keeps the box open
 * and says what is allowed instead of guessing.
 */
export default function ValueEntry({ text, name, parse, min, max, rangeText, onSet, className }: ValueEntryProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const classes = `value-entry ${className ?? ""}`;

  const cancel = () => {
    setDraft(null);
    setError(null);
  };
  const commit = () => {
    if (draft === null) return;
    const v = parse(draft);
    // A little slack so a typed "20000" isn't refused for rounding.
    const slack = (max - min) * 1e-6;
    if (v === null || !Number.isFinite(v)) {
      setError(`Type a number: ${rangeText}`);
    } else if (v < min - slack || v > max + slack) {
      setError(`Out of range: ${rangeText}`);
    } else {
      onSet(Math.min(max, Math.max(min, v)));
      cancel();
    }
  };

  if (draft === null) {
    const start = () => setDraft(text);
    return (
      <span
        className={classes}
        role="button"
        tabIndex={0}
        aria-label={`${name} value`}
        title="Double-click to type a value"
        onDoubleClick={(e) => {
          // Inside a label, a double-click would also reach the slider.
          e.preventDefault();
          e.stopPropagation();
          start();
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            start();
          }
        }}
      >
        {text}
      </span>
    );
  }

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancel();
    }
  };
  return (
    <span className={`${classes} editing`}>
      <input
        type="text"
        autoFocus
        aria-label={`Type ${name}`}
        aria-invalid={error !== null}
        value={draft}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => {
          setDraft(e.target.value);
          setError(null);
        }}
        onKeyDown={onKeyDown}
        // Clicking away keeps a valid value and drops an invalid one.
        onBlur={() => {
          const v = parse(draft);
          if (v !== null && v >= min && v <= max) commit();
          else cancel();
        }}
      />
      {error && (
        <span className="value-entry-error" role="alert">
          {error}
        </span>
      )}
    </span>
  );
}
