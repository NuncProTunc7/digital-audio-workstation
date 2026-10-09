import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

/** Space kept between a menu and the window edge, in px. */
const MARGIN = 8;

/**
 * Where to put a `width` × `height` menu opened at (x, y) so it stays inside
 * a `windowWidth` × `windowHeight` window: moved left or up as needed, and
 * never past the top-left corner.
 */
export function clampToWindow(
  x: number,
  y: number,
  width: number,
  height: number,
  windowWidth: number,
  windowHeight: number,
): { left: number; top: number } {
  const left = Math.max(MARGIN, Math.min(x, windowWidth - width - MARGIN));
  const top = Math.max(MARGIN, Math.min(y, windowHeight - height - MARGIN));
  return { left, top };
}

/**
 * The position for a `position: fixed` menu opened at (x, y), measured
 * after it renders and pulled back inside the window if it would be cut
 * off at an edge.
 */
export function useOnScreen(ref: RefObject<HTMLElement | null>, x: number, y: number): { left: number; top: number } {
  const [pos, setPos] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos(clampToWindow(x, y, width, height, window.innerWidth, window.innerHeight));
  }, [ref, x, y]);
  return pos;
}
