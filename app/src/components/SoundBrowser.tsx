import { useState } from "react";
import type { Catalog, InstrumentKind, Track, UserPreset } from "../types";

/** Describing words for the built-in sounds, for searching and filtering. */
const FACTORY_TAGS: Record<string, string[]> = {
  Init: ["basic"],
  "Warm Keys": ["warm", "soft", "keys", "chords", "cozy"],
  "Soft Pad": ["soft", "ambient", "warm", "pad", "calm"],
  "Bright Lead": ["bright", "lead", "melody"],
  Pluck: ["bright", "pluck", "arp", "light"],
  "Chip Square": ["retro", "chiptune", "8-bit", "lead"],
  "Brass Stab": ["punchy", "brass", "epic", "heroic"],
  "Sub Bass": ["deep", "clean", "bass", "dark"],
  "Fat Bass": ["fat", "bass", "warm"],
  "Acid Bass": ["retro", "aggressive", "bass", "electronic"],
  "Classic Kit": ["drums", "balanced"],
  "Tight Kit": ["drums", "tight", "fast", "dry"],
  "Boomy Kit": ["drums", "epic", "big", "boss"],
  "Sample pack": ["real", "orchestral", "acoustic", "piano"],
};

/** Filter chips offered above the list. */
const CHIPS = ["warm", "dark", "bright", "soft", "retro", "epic", "punchy", "bass", "drums", "pad", "lead", "orchestral"];

const KIND_NAMES: Record<InstrumentKind, string> = {
  synth: "Synth",
  drums: "Drums",
  sampler: "Sampler",
  audio: "Audio",
};

const FAVORITES_KEY = "npt.favoriteSounds";

function loadFavorites(): Set<string> {
  try {
    return new Set(JSON.parse(window.localStorage.getItem(FAVORITES_KEY) ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

function saveFavorites(f: Set<string>) {
  try {
    window.localStorage.setItem(FAVORITES_KEY, JSON.stringify([...f]));
  } catch {
    // Favorites are a convenience only.
  }
}

/** One sound in the browser. */
interface Sound {
  kind: InstrumentKind;
  name: string;
  description: string;
  tags: string[];
  mine: boolean;
}

interface SoundBrowserProps {
  catalog: Catalog;
  userPresets: UserPreset[];
  track: Track;
  /** Put the sound on the selected track and play a short phrase. */
  onTry: (sound: { kind: InstrumentKind; name: string; mine: boolean }) => void;
  /** Make a new track with the sound. */
  onNewTrack: (sound: { kind: InstrumentKind; name: string; mine: boolean }) => void;
}

const key = (s: Pick<Sound, "kind" | "name" | "mine">) => `${s.mine ? "user" : "factory"}:${s.kind}:${s.name}`;

/** Every built-in and saved sound, searchable by name, mood and role. */
export default function SoundBrowser(props: SoundBrowserProps) {
  const [query, setQuery] = useState("");
  const [chip, setChip] = useState<string | null>(null);
  const [onlyFavorites, setOnlyFavorites] = useState(false);
  const [favorites, setFavorites] = useState<Set<string>>(loadFavorites);

  const sounds: Sound[] = [
    ...props.catalog.instruments
      .filter((i) => i.kind !== "audio")
      .flatMap((i) =>
        i.presets.map((p) => ({
          kind: i.kind,
          name: p.name,
          description: p.description,
          tags: FACTORY_TAGS[p.name] ?? [],
          mine: false,
        })),
      ),
    ...props.userPresets.map((p) => ({
      kind: p.kind,
      name: p.name,
      description: "Your preset",
      // Your words count as tags too ("Dark Dungeon Pad" → dark, pad).
      tags: ["yours", ...CHIPS.filter((c) => p.name.toLowerCase().includes(c))],
      mine: true,
    })),
  ];
  const q = query.trim().toLowerCase();
  const shown = sounds.filter(
    (s) =>
      (!onlyFavorites || favorites.has(key(s))) &&
      (!chip || s.tags.includes(chip)) &&
      (!q || [s.name, s.description, KIND_NAMES[s.kind], ...s.tags].some((t) => t.toLowerCase().includes(q))),
  );

  const toggleFavorite = (s: Sound) => {
    const next = new Set(favorites);
    if (next.has(key(s))) next.delete(key(s));
    else next.add(key(s));
    setFavorites(next);
    saveFavorites(next);
  };

  return (
    <section className="sound-browser" aria-label="Sound browser">
      <div className="roll-toolbar">
        <input
          type="search"
          aria-label="Search sounds"
          placeholder="Search sounds: warm, bass, retro…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.stopPropagation()}
        />
        <div className="sound-chips" role="group" aria-label="Filter by mood">
          {CHIPS.map((c) => (
            <button
              key={c}
              className={chip === c ? "small toggle on" : "small toggle"}
              aria-pressed={chip === c}
              onClick={() => setChip(chip === c ? null : c)}
            >
              {c}
            </button>
          ))}
          <button
            className={onlyFavorites ? "small toggle on" : "small toggle"}
            aria-pressed={onlyFavorites}
            onClick={() => setOnlyFavorites((f) => !f)}
          >
            ★ Favorites
          </button>
        </div>
      </div>
      <ul className="sound-list">
        {shown.length === 0 && <li className="muted">No sounds match. Clear the search or the filter.</li>}
        {shown.map((s) => {
          const fits = s.kind === props.track.instrument.kind;
          return (
            <li key={key(s)} className="sound">
              <button
                className={favorites.has(key(s)) ? "tiny on favorite" : "tiny favorite"}
                aria-label={`${favorites.has(key(s)) ? "Unfavorite" : "Favorite"} ${s.name}`}
                aria-pressed={favorites.has(key(s))}
                onClick={() => toggleFavorite(s)}
              >
                ★
              </button>
              <div className="sound-text">
                <strong>{s.name}</strong> <span className="muted">· {KIND_NAMES[s.kind]}</span>
                <div className="muted sound-description">
                  {s.description}
                  {s.tags.length > 0 && <span className="sound-tags"> — {s.tags.join(", ")}</span>}
                </div>
              </div>
              {fits ? (
                <button
                  className="small"
                  onClick={() => props.onTry(s)}
                  title={`Put this sound on ${props.track.name} and hear it (Ctrl+Z puts the old one back)`}
                >
                  Try on {props.track.name}
                </button>
              ) : (
                <button className="small" onClick={() => props.onNewTrack(s)} title="Add a new track with this sound">
                  + New track
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
