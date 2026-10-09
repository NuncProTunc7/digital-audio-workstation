import { useRef } from "react";
import type { HTMLAttributes } from "react";
import { useOnScreen } from "../placement";

interface FixedMenuProps extends Omit<HTMLAttributes<HTMLDivElement>, "style"> {
  /** Where it was opened, in window coordinates. */
  x: number;
  y: number;
}

/** A pop-up menu at a click point that moves back inside the window
 * instead of being cut off at the right or bottom edge. */
export default function FixedMenu({ x, y, children, ...rest }: FixedMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { left, top } = useOnScreen(ref, x, y);
  return (
    <div ref={ref} {...rest} style={{ left, top }}>
      {children}
    </div>
  );
}
