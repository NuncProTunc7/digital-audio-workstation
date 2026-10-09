import { useState } from "react";
import type { Catalog, LibraryPack, PluginList, PluginParam, PluginStatus, SamplePackStatus, Track, UserPreset } from "../types";
import DrumPads from "./DrumPads";
import ParamControl from "./ParamControl";
import PluginChooser from "./PluginChooser";
import PluginPanel from "./PluginPanel";
import SamplePack from "./SamplePack";

interface InstrumentPanelProps {
  track: Track;
  catalog: Catalog;
  activeNotes: ReadonlySet<number>;
  onParam: (param: string, value: number) => void;
  onEndGesture: () => void;
  onPreset: (preset: string) => void;
  /** The user's own presets for this track's kind of instrument. */
  userPresets: UserPreset[];
  onUserPreset: (name: string) => void;
  onSavePreset: (name: string) => void;
  onDeletePreset: (name: string) => void;
  onPadHit: (note: number) => void;
  onPadRelease: (note: number) => void;
  onChooseSamplePack: () => void;
  samplePackStatus: (path: string) => Promise<SamplePackStatus>;
  sampleLibrary?: () => Promise<LibraryPack[]>;
  onDownloadSamplePack?: (id: string) => Promise<void>;
  onUseSamplePack?: (path: string, label: string) => void;
  /** Third-party plugins; omitted where they can't be used. */
  plugins?: {
    list: (rescan: boolean) => Promise<PluginList>;
    status: (trackId: number) => Promise<PluginStatus>;
    params: (trackId: number) => Promise<PluginParam[]>;
    onLoad: (uid: string) => void;
    onOpenWindow: () => void;
    onParam: (id: number, value: number) => void;
  };
}

/** Preset picker plus every parameter of the selected track's instrument. */
export default function InstrumentPanel({
  track,
  catalog,
  activeNotes,
  onParam,
  onEndGesture,
  onPreset,
  userPresets,
  onUserPreset,
  onSavePreset,
  onDeletePreset,
  onPadHit,
  onPadRelease,
  onChooseSamplePack,
  samplePackStatus,
  sampleLibrary,
  onDownloadSamplePack,
  onUseSamplePack,
  plugins,
}: InstrumentPanelProps) {
  const [saving, setSaving] = useState(false);
  const chooser = plugins && track.instrument.kind !== "audio" && (
    <PluginChooser list={plugins.list} current={track.instrument.plugin?.uid} onChoose={plugins.onLoad} />
  );
  if (track.instrument.kind === "plugin") {
    return (
      <section className="instrument-panel" aria-label={`${track.name} instrument`}>
        <header className="instrument-header">
          <h2>{track.name}</h2>
          <span className="instrument-kind">Plugin</span>
          {chooser}
        </header>
        <div className="instrument-body plugin">
          {plugins ? (
            <PluginPanel
              track={track}
              status={plugins.status}
              params={plugins.params}
              onOpenWindow={plugins.onOpenWindow}
              onParam={plugins.onParam}
              onEndGesture={onEndGesture}
            />
          ) : (
            <p className="muted">Plugins need the desktop app.</p>
          )}
        </div>
      </section>
    );
  }
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
  const mine = userPresets.find((p) => p.name === track.instrument.preset);
  // User presets are told apart from built-in ones by this prefix.
  const USER = "user:";
  const value = mine ? USER + mine.name : track.instrument.preset;

  return (
    <section className="instrument-panel" aria-label={`${track.name} instrument`}>
      <header className="instrument-header">
        <h2>{track.name}</h2>
        <span className="instrument-kind">
          {track.instrument.kind === "drums" ? "Drum machine" : track.instrument.kind === "sampler" ? "Sampler" : "Synth"}
        </span>
        <label className="field">
          <span>Preset</span>
          <select
            aria-label="Preset"
            value={value}
            onChange={(e) => {
              const v = e.target.value;
              if (v.startsWith(USER)) onUserPreset(v.slice(USER.length));
              else onPreset(v);
              // Hand the keyboard back to musical typing.
              e.currentTarget.blur();
            }}
          >
            {!preset && !mine && <option>{track.instrument.preset}</option>}
            <optgroup label="Built-in">
              {description.presets.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
            </optgroup>
            {userPresets.length > 0 && (
              <optgroup label="Your presets">
                {userPresets.map((p) => (
                  <option key={p.name} value={USER + p.name}>
                    {p.name}
                  </option>
                ))}
              </optgroup>
            )}
          </select>
        </label>
        {track.instrument.kind !== "audio" &&
          (saving ? (
            <form
              className="preset-save"
              onSubmit={(e) => {
                e.preventDefault();
                const input = e.currentTarget.elements.namedItem("preset-name") as HTMLInputElement;
                const name = input.value.trim();
                if (name) onSavePreset(name);
                setSaving(false);
              }}
            >
              <input
                name="preset-name"
                aria-label="New preset name"
                autoFocus
                defaultValue={mine ? mine.name : `My ${track.instrument.preset}`}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Escape") setSaving(false);
                }}
              />
              <button type="submit" className="small" aria-label="Save this preset">
                Save
              </button>
            </form>
          ) : (
            <button
              className="small"
              onClick={() => setSaving(true)}
              title="Save this sound under your own name, for any song"
            >
              Save preset…
            </button>
          ))}
        {mine && (
          <button
            className="small"
            aria-label={`Delete preset ${mine.name}`}
            title="Delete this saved preset (tracks using it keep their sound)"
            onClick={() => onDeletePreset(mine.name)}
          >
            ✕
          </button>
        )}
        {chooser}
        {preset && <span className="preset-description">{preset.description}</span>}
      </header>

      <div className={`instrument-body ${track.instrument.kind}`}>
        {track.instrument.kind === "drums" && (
          <DrumPads pads={catalog.drum_pads} activeNotes={activeNotes} onHit={onPadHit} onRelease={onPadRelease} />
        )}
        <div className="param-groups">
          {track.instrument.kind === "sampler" && (
            <SamplePack
              track={track}
              onChoose={onChooseSamplePack}
              status={samplePackStatus}
              library={sampleLibrary}
              onDownload={onDownloadSamplePack}
              onUse={onUseSamplePack}
            />
          )}
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
