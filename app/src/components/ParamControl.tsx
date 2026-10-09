import { useState } from "react";
import { formatParam, fromSlider, parseParam, SLIDER_STEPS, toSlider } from "../format";
import type { ParamSpec } from "../types";
import ValueEntry from "./ValueEntry";

interface ParamControlProps {
  spec: ParamSpec;
  value: number;
  onChange: (value: number) => void;
  /** Called when a drag ends, so the next drag is its own undo step. */
  onCommit: () => void;
}

/**
 * One instrument parameter: a slider (log-scaled where it helps) or, for
 * choices, a row of buttons. Double-click a slider to reset it, or its
 * value to type one.
 */
export default function ParamControl({ spec, value, onChange, onCommit }: ParamControlProps) {
  // While dragging, show the local value so the slider never lags the engine.
  const [dragValue, setDragValue] = useState<number | null>(null);
  const shown = dragValue ?? value;

  if (spec.choices.length > 0) {
    return (
      <div className="param choice">
        <span className="param-name">{spec.name}</span>
        <div className="choices" role="radiogroup" aria-label={spec.name}>
          {spec.choices.map((choice, index) => (
            <button
              key={choice}
              role="radio"
              aria-checked={Math.round(value) === index}
              className={Math.round(value) === index ? "selected" : ""}
              onClick={() => {
                onChange(index);
                onCommit();
              }}
            >
              {choice}
            </button>
          ))}
        </div>
      </div>
    );
  }

  const finish = () => {
    if (dragValue !== null) {
      setDragValue(null);
      onCommit();
    }
  };

  return (
    <label className="param">
      <span className="param-name">{spec.name}</span>
      <input
        type="range"
        min={0}
        max={SLIDER_STEPS}
        value={toSlider(spec, shown)}
        aria-label={spec.name}
        onChange={(e) => {
          const v = fromSlider(spec, Number(e.target.value));
          setDragValue(v);
          onChange(v);
        }}
        onPointerUp={finish}
        onKeyUp={finish}
        onBlur={finish}
        onDoubleClick={() => {
          onChange(spec.default);
          onCommit();
        }}
      />
      <ValueEntry
        className="param-value"
        text={formatParam(spec, shown)}
        name={spec.name}
        parse={(text) => parseParam(spec, text, shown)}
        min={spec.min}
        max={spec.max}
        rangeText={`${formatParam(spec, spec.min)} to ${formatParam(spec, spec.max)}`}
        onSet={(v) => {
          onChange(v);
          onCommit();
        }}
      />
    </label>
  );
}
