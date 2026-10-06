import type { Catalog, Track } from "../types";
import DrumPads from "./DrumPads";
import ParamControl from "./ParamControl";

interface InstrumentPanelProps {
  track: Track;
  catalog: Catalog;
  activeNotes: ReadonlySet<number>;
  onParam: (param: string, value: number) => void;
  onEndGesture: () => void;
  onPreset: (preset: string) => void;
  onPadHit: (note: number) => void;
  onPadRelease: (note: number) => void;
}

/** Preset picker plus every parameter of the selected track's instrument. */
export default function InstrumentPanel({
  track,
  catalog,
  activeNotes,
  onParam,
  onEndGesture,
  onPreset,
  onPadHit,
  onPadRelease,
}: InstrumentPanelProps) {
  const description = catalog.instruments.find((i) => i.kind === track.instrument.kind);
  if (!description) return null;

  // Group parameters by section, keeping the catalog's order.
  const groups = new Map<string, typeof description.params>();
  for (const spec of description.params) {
    const list = groups.get(spec.group) ?? [];
    list.push(spec);
    groups.set(spec.group, list);
  }
  const preset = description.presets.find((p) => p.name === track.instrument.preset);

  return (
    <section className="instrument-panel" aria-label={`${track.name} instrument`}>
      <header className="instrument-header">
        <h2>{track.name}</h2>
        <span className="instrument-kind">{track.instrument.kind === "drums" ? "Drum machine" : "Synth"}</span>
        <label className="field">
          <span>Preset</span>
          <select
            aria-label="Preset"
            value={track.instrument.preset}
            onChange={(e) => {
              onPreset(e.target.value);
              // Hand the keyboard back to musical typing.
              e.currentTarget.blur();
            }}
          >
            {!preset && <option>{track.instrument.preset}</option>}
            {description.presets.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        {preset && <span className="preset-description">{preset.description}</span>}
      </header>

      <div className={`instrument-body ${track.instrument.kind}`}>
        {track.instrument.kind === "drums" && (
          <DrumPads pads={catalog.drum_pads} activeNotes={activeNotes} onHit={onPadHit} onRelease={onPadRelease} />
        )}
        <div className="param-groups">
          {[...groups.entries()].map(([group, specs]) => (
            <fieldset key={group} className="param-group">
              <legend>{group}</legend>
              {specs.map((spec) => (
                <ParamControl
                  key={spec.id}
                  spec={spec}
                  value={track.instrument.params[spec.id] ?? spec.default}
                  onChange={(v) => onParam(spec.id, v)}
                  onCommit={onEndGesture}
                />
              ))}
            </fieldset>
          ))}
        </div>
      </div>
    </section>
  );
}
