import { useEffect, useRef } from "react";
import type { Peaks } from "../types";

/** Canvases wider than this are drawn at this width and stretched. */
const MAX_CANVAS_PX = 4096;

interface WaveformProps {
  peaks: Peaks;
  /** Where in the file the clip starts, and how much of it shows. */
  offsetSeconds: number;
  seconds: number;
  /** Linear clip gain, so turning a clip up draws it bigger. */
  gain: number;
  width: number;
  height: number;
}

/** Min/max waveform of part of an audio file. */
export default function Waveform({ peaks, offsetSeconds, seconds, gain, width, height }: WaveformProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const pixels = Math.max(1, Math.min(MAX_CANVAS_PX, Math.round(width)));

  useEffect(() => {
    const canvas = ref.current;
    // jsdom (tests) has no 2D context.
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const h = canvas.height;
    const mid = h / 2;
    ctx.clearRect(0, 0, pixels, h);
    ctx.fillStyle = getComputedStyle(canvas).color || "#fff";
    const buckets = peaks.min_max.length / 2;
    const perPixel = (seconds / pixels) * peaks.per_second;
    for (let x = 0; x < pixels; x++) {
      const from = Math.floor((offsetSeconds + (x / pixels) * seconds) * peaks.per_second);
      const to = Math.max(from + 1, Math.floor(from + perPixel));
      if (from >= buckets) break;
      let lo = 0;
      let hi = 0;
      for (let b = Math.max(0, from); b < Math.min(buckets, to); b++) {
        lo = Math.min(lo, peaks.min_max[2 * b]);
        hi = Math.max(hi, peaks.min_max[2 * b + 1]);
      }
      const top = mid - Math.min(1, hi * gain) * mid;
      const bottom = mid - Math.max(-1, lo * gain) * mid;
      ctx.fillRect(x, top, 1, Math.max(1, bottom - top));
    }
  }, [peaks, offsetSeconds, seconds, gain, pixels, height]);

  return <canvas ref={ref} className="waveform" width={pixels} height={Math.max(1, Math.round(height))} style={{ width, height }} aria-hidden />;
}
