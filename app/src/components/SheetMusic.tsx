import { useEffect, useRef, useState } from "react";
import type { OpenSheetMusicDisplay } from "opensheetmusicdisplay";

interface SheetMusicProps {
  /** MusicXML to show; null while it is being made. */
  xml: string | null;
  zoom: number;
}

/** Renders a MusicXML score with OpenSheetMusicDisplay (loaded on first use). */
export default function SheetMusic({ xml, zoom }: SheetMusicProps) {
  const host = useRef<HTMLDivElement>(null);
  const osmd = useRef<OpenSheetMusicDisplay | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!xml || !host.current) return;
    let cancelled = false;
    void (async () => {
      try {
        if (!osmd.current) {
          const { OpenSheetMusicDisplay } = await import("opensheetmusicdisplay");
          if (cancelled || !host.current) return;
          osmd.current = new OpenSheetMusicDisplay(host.current, {
            autoResize: true,
            backend: "svg",
            drawTitle: false,
            drawPartNames: true,
            autoBeam: true,
          });
        }
        await osmd.current.load(xml);
        if (cancelled) return;
        osmd.current.zoom = zoom;
        osmd.current.render();
        setError(null);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [xml, zoom]);

  return (
    <div className="sheet-music">
      {error && (
        <p className="muted" role="alert">
          The score couldn't be drawn: {error}
        </p>
      )}
      <div ref={host} className="sheet-music-page" aria-label="Sheet music" />
    </div>
  );
}
