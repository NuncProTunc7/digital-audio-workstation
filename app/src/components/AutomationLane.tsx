import { useRef } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { snap } from "../format";
import { AUTOMATION_HEIGHT } from "../automation";
import type { TargetScale } from "../automation";
import type { AutomationLane as Lane, AutomationPoint } from "../types";

/** Points sit this far inside the lane's top and bottom edges. */
const PAD = 6;

interface AutomationLaneProps {
  lane: Lane;
  scale: TargetScale;
  ppb: number;
  width: number;
  grid: number;
  onPoints: (points: AutomationPoint[]) => void;
  onEndGesture: () => void;
}

/** A lane's curve: click to add a point, drag to move, double-click to remove. */
export default function AutomationLaneEditor({ lane, scale, ppb, width, grid, onPoints, onEndGesture }: AutomationLaneProps) {
  const svg = useRef<SVGSVGElement>(null);
  const drag = useRef<{ index: number; points: AutomationPoint[] } | null>(null);
  const inner = AUTOMATION_HEIGHT - 2 * PAD;

  const toY = (v: number) => {
    const t = scale.log
      ? Math.log(v / scale.min) / Math.log(scale.max / scale.min)
      : (v - scale.min) / (scale.max - scale.min);
    return PAD + (1 - Math.max(0, Math.min(1, t))) * inner;
  };
  const fromY = (y: number) => {
    const t = 1 - Math.max(0, Math.min(1, (y - PAD) / inner));
    const v = scale.log ? scale.min * (scale.max / scale.min) ** t : scale.min + t * (scale.max - scale.min);
    return Math.max(scale.min, Math.min(scale.max, v));
  };
  const at = (e: { clientX: number; clientY: number }) => {
    const r = svg.current?.getBoundingClientRect();
    const x = r ? e.clientX - r.left : 0;
    const y = r ? e.clientY - r.top : 0;
    return { beats: Math.max(0, snap(x / ppb, grid)), value: fromY(y) };
  };

  const startDrag = (e: ReactPointerEvent, index: number, points: AutomationPoint[]) => {
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { index, points };
  };

  const points = lane.points;
  const path =
    points.length === 0
      ? ""
      : [
          `M 0 ${toY(points[0].value)}`,
          ...points.map((p) => `L ${p.beats * ppb} ${toY(p.value)}`),
          `L ${width} ${toY(points[points.length - 1].value)}`,
        ].join(" ");

  return (
    <svg
      ref={svg}
      className={lane.enabled ? "automation-lane" : "automation-lane off"}
      width={width}
      height={AUTOMATION_HEIGHT}
      aria-label={`${scale.label} automation`}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const p = at(e);
        const next = [...points, p].sort((a, b) => a.beats - b.beats);
        onPoints(next);
        startDrag(e, next.indexOf(p), next);
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const moved = at(e);
        const next = d.points.map((p, i) => (i === d.index ? moved : p));
        onPoints([...next].sort((a, b) => a.beats - b.beats));
      }}
      onPointerUp={() => {
        if (drag.current) {
          drag.current = null;
          onEndGesture();
        }
      }}
    >
      <path d={path} className="automation-line" />
      {points.map((p, i) => (
        <circle
          key={`${i}-${p.beats}`}
          cx={p.beats * ppb}
          cy={toY(p.value)}
          r={4}
          className="automation-point"
          data-point={i}
          onPointerDown={(e) => startDrag(e, i, points)}
          onDoubleClick={(e) => {
            e.stopPropagation();
            onPoints(points.filter((_, j) => j !== i));
            onEndGesture();
          }}
        >
          <title>
            {scale.label} {scale.format(p.value)} at beat {p.beats + 1}
          </title>
        </circle>
      ))}
    </svg>
  );
}
