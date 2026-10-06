import { useRef } from "react";
import type { KeyboardEvent, PointerEvent } from "react";

interface FaderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  /** Value restored on double-click. */
  defaultValue: number;
  onChange: (value: number) => void;
  onCommit: () => void;
}

/**
 * Vertical volume fader. Drawn by hand (not a rotated range input) so it
 * looks and behaves the same in every webview. Drag, use arrow keys, or
 * double-click to reset.
 */
export default function Fader({ label, value, min, max, defaultValue, onChange, onCommit }: FaderProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const fraction = (value - min) / (max - min);

  const valueAt = (clientY: number) => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect || rect.height === 0) return value;
    const t = 1 - (clientY - rect.top) / rect.height;
    const v = min + Math.max(0, Math.min(1, t)) * (max - min);
    return Math.round(v * 10) / 10;
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture?.(e.pointerId);
    dragging.current = true;
    onChange(valueAt(e.clientY));
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (dragging.current) onChange(valueAt(e.clientY));
  };
  const onPointerUp = () => {
    if (dragging.current) {
      dragging.current = false;
      onCommit();
    }
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 0.1 : 1;
    const delta = e.key === "ArrowUp" ? step : e.key === "ArrowDown" ? -step : 0;
    if (delta !== 0) {
      e.preventDefault();
      e.stopPropagation();
      onChange(Math.max(min, Math.min(max, Math.round((value + delta) * 10) / 10)));
      onCommit();
    }
  };

  return (
    <div
      className="fader"
      ref={trackRef}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onKeyDown={onKeyDown}
      onDoubleClick={() => {
        onChange(defaultValue);
        onCommit();
      }}
      title={`${label} (double-click to reset)`}
    >
      <div className="fader-fill" style={{ height: `${fraction * 100}%` }} />
      <div className="fader-thumb" style={{ bottom: `calc(${fraction * 100}% - 5px)` }} />
    </div>
  );
}
