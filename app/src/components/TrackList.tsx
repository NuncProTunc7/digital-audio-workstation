import type { Track } from "../types";

interface TrackListProps {
  tracks: Track[];
  selectedId: number;
  onSelect: (id: number) => void;
}

export default function TrackList({ tracks, selectedId, onSelect }: TrackListProps) {
  return (
    <aside className="track-list">
      <div className="panel-title">Tracks</div>
      <ul role="listbox" aria-label="Tracks">
        {tracks.map((t) => (
          <li key={t.id}>
            <button
              role="option"
              aria-selected={t.id === selectedId}
              className={t.id === selectedId ? "track selected" : "track"}
              onClick={() => onSelect(t.id)}
            >
              <span className="track-icon" aria-hidden>
                {t.instrument.kind === "drums" ? "🥁" : "🎹"}
              </span>
              <span className="track-text">
                <span className="track-name">{t.name}</span>
                <span className="track-preset">{t.instrument.preset}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
      <p className="hint">Phase 2 adds new tracks, clips, and the timeline.</p>
    </aside>
  );
}
