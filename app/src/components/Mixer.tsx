import { createContext, useContext, useEffect, useRef, useState } from "react";
import { formatDb, meterPercent, parseDb, parsePan } from "../format";
import type { Bus, Catalog, Command, Effect, EffectKind, PluginInfo, PluginList, Project, Track } from "../types";
import Fader from "./Fader";
import ParamControl from "./ParamControl";
import ValueEntry from "./ValueEntry";

interface MixerProps {
  project: Project;
  catalog: Catalog;
  trackPeaks: number[];
  busPeaks: number[];
  masterPeaks: [number, number];
  selectedTrackId: number;
  onSelectTrack: (id: number) => void;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
  /** Third-party plugin effects; omitted where they can't be used. */
  plugins?: {
    list: (rescan: boolean) => Promise<PluginList>;
    /** Adds a plugin effect to a track's, bus's or (null) the master's chain. */
    add: (trackId: number | null, uid: string) => void;
    openWindow: (effectId: number) => void;
  };
}

/** Plugin effects the strips and effect cards offer. */
const PluginEffects = createContext<{
  effects: PluginInfo[];
  add: (trackId: number | null, uid: string) => void;
  openWindow: (effectId: number) => void;
} | null>(null);

const PLUGIN = "plugin:";

/** "C", "30L" or "45R". */
function formatPan(pan: number): string {
  return pan === 0 ? "C" : `${Math.round(Math.abs(pan) * 100)}${pan < 0 ? "L" : "R"}`;
}

/** A pan readout you can type into ("30L", "C"). */
function PanEntry({ name, pan, onSet }: { name: string; pan: number; onSet: (pan: number) => void }) {
  return (
    <ValueEntry
      className="param-value"
      text={formatPan(pan)}
      name={`${name} pan`}
      parse={parsePan}
      min={-1}
      max={1}
      rangeText="100L to 100R (C is centre)"
      onSet={onSet}
    />
  );
}

/** Where a track plays, and what it sends to other buses. */
function Routing({ track, buses, onCommand, onEndGesture }: {
  track: Track;
  buses: Bus[];
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
}) {
  if (buses.length === 0) return null;
  const run = (command: Command) => void onCommand(command).then(onEndGesture);
  const sends = track.sends ?? [];
  return (
    <div className="routing">
      <label className="routing-out" title="Where this track plays: the master, or a group bus">
        <span>Out</span>
        <select
          aria-label={`${track.name} output`}
          value={track.output ?? ""}
          onChange={(e) => {
            run({ command: "set_track_output", track_id: track.id, bus_id: e.target.value === "" ? null : Number(e.target.value) });
            e.currentTarget.blur();
          }}
        >
          <option value="">Master</option>
          {buses.map((b) => (
            <option key={b.id} value={b.id}>
              {b.name}
            </option>
          ))}
        </select>
      </label>
      {buses
        .filter((b) => b.id !== track.output)
        .map((b) => {
          const s = sends.find((x) => x.bus_id === b.id);
          return (
            <div className="send" key={b.id} title={`Send some of ${track.name} to ${b.name} as well`}>
              <label>
                <input
                  type="checkbox"
                  aria-label={`Send ${track.name} to ${b.name}`}
                  checked={s !== undefined}
                  onChange={() =>
                    run(
                      s
                        ? { command: "remove_send", track_id: track.id, bus_id: b.id }
                        : { command: "set_send", track_id: track.id, bus_id: b.id, level_db: null, pre_fader: null },
                    )
                  }
                />
                <span>→ {b.name}</span>
              </label>
              {s && (
                <ValueEntry
                  className="send-db"
                  text={`${s.level_db.toFixed(1)} dB`}
                  name={`${track.name} send level`}
                  parse={parseDb}
                  min={-60}
                  max={6}
                  rangeText="-60 to +6 dB"
                  onSet={(v) => run({ command: "set_send", track_id: track.id, bus_id: b.id, level_db: v, pre_fader: null })}
                />
              )}
              {s && (
                <input
                  type="range"
                  min={-60}
                  max={6}
                  step={0.5}
                  value={s.level_db}
                  aria-label={`${track.name} send to ${b.name}`}
                  title={`${s.level_db.toFixed(1)} dB`}
                  onChange={(e) =>
                    void onCommand({
                      command: "set_send",
                      track_id: track.id,
                      bus_id: b.id,
                      level_db: Number(e.target.value),
                      pre_fader: null,
                    })
                  }
                  onPointerUp={onEndGesture}
                  onKeyUp={onEndGesture}
                />
              )}
            </div>
          );
        })}
    </div>
  );
}

/** Channel strips for every track, the group buses, and the master. */
export default function Mixer(props: MixerProps) {
  const { project } = props;
  const buses = project.buses ?? [];
  const send = (command: Command) => void props.onCommand(command);
  const [pluginEffects, setPluginEffects] = useState<PluginInfo[]>([]);
  const list = props.plugins?.list;
  useEffect(() => {
    if (!list) return;
    void list(false)
      .then((l) => setPluginEffects(l.plugins.filter((p) => p.kind === "effect")))
      .catch(() => {});
  }, [list]);
  const plugins = props.plugins ? { effects: pluginEffects, add: props.plugins.add, openWindow: props.plugins.openWindow } : null;
  // A plain mouse wheel scrolls the strips sideways (most mice can't scroll
  // sideways). A native listener, because React's wheel listener is passive
  // and can't stop the page scrolling as well.
  const stripsRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = stripsRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || el.scrollWidth <= el.clientWidth) return;
      // Leave the wheel to an effect list or strip that scrolls up and down.
      for (let n = e.target as HTMLElement | null; n && n !== el; n = n.parentElement) {
        if (n.scrollHeight > n.clientHeight && ["auto", "scroll"].includes(getComputedStyle(n).overflowY)) return;
      }
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);
  const mixer = (
    <div className="mixer">
      <div className="mixer-strips" role="group" aria-label="Track and bus channels" ref={stripsRef}>
        {project.tracks.map((t, i) => (
          <Strip
            tracks={project.tracks}
            key={t.id}
            name={t.name}
            selected={t.id === props.selectedTrackId}
            onSelect={() => props.onSelectTrack(t.id)}
            volumeDb={t.mixer.volume_db}
            peak={props.trackPeaks[i] ?? 0}
            effects={t.mixer.effects}
            trackId={t.id}
            catalog={props.catalog}
            onCommand={props.onCommand}
            onEndGesture={props.onEndGesture}
            onVolume={(v) =>
              send({ command: "set_track_mixer", track_id: t.id, volume_db: v, pan: null, mute: null, solo: null })
            }
          >
            <label className="pan" title="Pan (double-click to center)">
              <span>Pan</span>
              <input
                type="range"
                min={-100}
                max={100}
                value={Math.round(t.mixer.pan * 100)}
                aria-label={`${t.name} pan`}
                onChange={(e) =>
                  send({
                    command: "set_track_mixer",
                    track_id: t.id,
                    volume_db: null,
                    pan: Number(e.target.value) / 100,
                    mute: null,
                    solo: null,
                  })
                }
                onPointerUp={props.onEndGesture}
                onDoubleClick={() => {
                  send({ command: "set_track_mixer", track_id: t.id, volume_db: null, pan: 0, mute: null, solo: null });
                  props.onEndGesture();
                }}
              />
              <PanEntry
                name={t.name}
                pan={t.mixer.pan}
                onSet={(pan) =>
                  void props
                    .onCommand({ command: "set_track_mixer", track_id: t.id, volume_db: null, pan, mute: null, solo: null })
                    .then(props.onEndGesture)
                }
              />
            </label>
            <div className="strip-buttons">
              <button
                className={t.mixer.mute ? "tiny on mute" : "tiny"}
                aria-pressed={t.mixer.mute}
                onClick={() => {
                  send({ command: "set_track_mixer", track_id: t.id, volume_db: null, pan: null, mute: !t.mixer.mute, solo: null });
                  props.onEndGesture();
                }}
              >
                M
              </button>
              <button
                className={t.mixer.solo ? "tiny on solo" : "tiny"}
                aria-pressed={t.mixer.solo}
                onClick={() => {
                  send({ command: "set_track_mixer", track_id: t.id, volume_db: null, pan: null, mute: null, solo: !t.mixer.solo });
                  props.onEndGesture();
                }}
              >
                S
              </button>
            </div>
            <Routing track={t} buses={buses} onCommand={props.onCommand} onEndGesture={props.onEndGesture} />
          </Strip>
        ))}
        {buses.map((b, i) => (
          <Strip
            tracks={project.tracks}
            key={b.id}
            name={b.name}
            bus
            selected={false}
            onSelect={() => {}}
            volumeDb={b.mixer.volume_db}
            peak={props.busPeaks[i] ?? 0}
            effects={b.mixer.effects}
            trackId={b.id}
            catalog={props.catalog}
            onCommand={props.onCommand}
            onEndGesture={props.onEndGesture}
            onVolume={(v) => send({ command: "set_bus_mixer", bus_id: b.id, volume_db: v, pan: null, mute: null })}
            onRename={(name) =>
              void props.onCommand({ command: "rename_bus", bus_id: b.id, name }).then(props.onEndGesture)
            }
          >
            <label className="pan" title="Pan (double-click to center)">
              <span>Pan</span>
              <input
                type="range"
                min={-100}
                max={100}
                value={Math.round(b.mixer.pan * 100)}
                aria-label={`${b.name} pan`}
                onChange={(e) =>
                  send({ command: "set_bus_mixer", bus_id: b.id, volume_db: null, pan: Number(e.target.value) / 100, mute: null })
                }
                onPointerUp={props.onEndGesture}
                onDoubleClick={() => {
                  send({ command: "set_bus_mixer", bus_id: b.id, volume_db: null, pan: 0, mute: null });
                  props.onEndGesture();
                }}
              />
              <PanEntry
                name={b.name}
                pan={b.mixer.pan}
                onSet={(pan) =>
                  void props
                    .onCommand({ command: "set_bus_mixer", bus_id: b.id, volume_db: null, pan, mute: null })
                    .then(props.onEndGesture)
                }
              />
            </label>
            <div className="strip-buttons">
              <button
                className={b.mixer.mute ? "tiny on mute" : "tiny"}
                aria-pressed={b.mixer.mute}
                aria-label={`Mute ${b.name}`}
                onClick={() => {
                  send({ command: "set_bus_mixer", bus_id: b.id, volume_db: null, pan: null, mute: !b.mixer.mute });
                  props.onEndGesture();
                }}
              >
                M
              </button>
              <button
                className="tiny"
                aria-label={`Delete bus ${b.name}`}
                title="Delete this bus (its tracks play into the master)"
                onClick={() => void props.onCommand({ command: "remove_bus", bus_id: b.id }).then(props.onEndGesture)}
              >
                ✕
              </button>
            </div>
            <div className="bus-members muted">
              {project.tracks
                .filter((t) => t.output === b.id)
                .map((t) => t.name)
                .join(", ") || "No tracks yet"}
            </div>
          </Strip>
        ))}
      </div>
      {/* Always in view, however many tracks there are. */}
      <div className="mixer-pinned">
        <button
          className="add-bus"
          onClick={() =>
            void props
              .onCommand({ command: "add_bus", name: buses.length === 0 ? "Reverb" : `Bus ${buses.length + 1}` })
              .then(props.onEndGesture)
          }
          title="Add a group bus: send several tracks to one shared reverb, or group drums to set their level together"
        >
          + Bus
        </button>
        <Strip
          tracks={project.tracks}
          name="Master"
          master
          selected={false}
          onSelect={() => {}}
          volumeDb={project.master.volume_db}
          peak={Math.max(...props.masterPeaks)}
          effects={project.master.effects}
          trackId={null}
          catalog={props.catalog}
          onCommand={props.onCommand}
          onEndGesture={props.onEndGesture}
          onVolume={(v) => send({ command: "set_master_volume", volume_db: v })}
        />
      </div>
    </div>
  );
  return <PluginEffects.Provider value={plugins}>{mixer}</PluginEffects.Provider>;
}

interface StripProps {
  /** Every track, for compressors that listen to one (sidechain). */
  tracks: Track[];
  name: string;
  master?: boolean;
  bus?: boolean;
  /** Buses can be renamed by double-clicking their name. */
  onRename?: (name: string) => void;
  selected: boolean;
  onSelect: () => void;
  volumeDb: number;
  peak: number;
  effects: Effect[];
  trackId: number | null;
  catalog: Catalog;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
  onVolume: (db: number) => void;
  children?: React.ReactNode;
}

function Strip(props: StripProps) {
  const [open, setOpen] = useState<number | null>(null);
  const [renaming, setRenaming] = useState(false);
  const plugins = useContext(PluginEffects);
  return (
    <section
      className={`strip${props.master ? " master" : ""}${props.bus ? " bus" : ""}${props.selected ? " selected" : ""}`}
      aria-label={`${props.name} channel`}
      onPointerDown={props.onSelect}
    >
      {renaming && props.onRename ? (
        <input
          className="strip-name"
          aria-label="Bus name"
          autoFocus
          defaultValue={props.name}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") setRenaming(false);
          }}
          onBlur={(e) => {
            setRenaming(false);
            const name = e.target.value.trim();
            if (name && name !== props.name) props.onRename?.(name);
          }}
        />
      ) : (
        <header
          className="strip-name"
          onDoubleClick={() => props.onRename && setRenaming(true)}
          title={props.onRename ? "Double-click to rename" : undefined}
        >
          {props.bus ? "⇶ " : ""}
          {props.name}
        </header>
      )}

      <div className="effects">
        {props.effects.map((e) => (
          <EffectCard
            key={e.id}
            tracks={props.tracks}
            effect={e}
            trackId={props.trackId}
            catalog={props.catalog}
            open={open === e.id}
            onToggleOpen={() => setOpen(open === e.id ? null : e.id)}
            onCommand={props.onCommand}
            onEndGesture={props.onEndGesture}
          />
        ))}
        <select
          aria-label={`Add effect to ${props.name}`}
          value=""
          onChange={(e) => {
            const v = e.target.value;
            e.currentTarget.blur();
            if (v.startsWith(PLUGIN)) {
              plugins?.add(props.trackId, v.slice(PLUGIN.length));
              return;
            }
            void props
              .onCommand({ command: "add_effect", track_id: props.trackId, kind: v as EffectKind, index: null })
              .then(props.onEndGesture);
          }}
        >
          <option value="">+ Add effect…</option>
          <optgroup label="Built-in">
            {props.catalog.effects.map((d) => (
              <option key={d.kind} value={d.kind} title={d.description}>
                {d.name}
              </option>
            ))}
          </optgroup>
          {plugins && plugins.effects.length > 0 && (
            <optgroup label="Plugins">
              {plugins.effects.map((p) => (
                <option key={p.uid} value={PLUGIN + p.uid} title={p.categories}>
                  {p.name} ({p.vendor})
                </option>
              ))}
            </optgroup>
          )}
        </select>
      </div>

      {props.children}

      <div className="fader-area">
        <Fader
          label={`${props.name} volume`}
          value={props.volumeDb}
          min={-60}
          max={6}
          defaultValue={0}
          onChange={props.onVolume}
          onCommit={props.onEndGesture}
        />
        <div className="strip-meter" aria-hidden>
          <div style={{ height: `${meterPercent(props.peak)}%` }} className={props.peak > 0.9 ? "hot" : ""} />
        </div>
      </div>
      <ValueEntry
        className="strip-db"
        text={formatDb(props.volumeDb)}
        name={`${props.name} volume`}
        parse={parseDb}
        min={-60}
        max={6}
        rangeText="-60 to +6 dB"
        onSet={(v) => {
          props.onVolume(v);
          props.onEndGesture();
        }}
      />
    </section>
  );
}

interface EffectCardProps {
  tracks: Track[];
  effect: Effect;
  trackId: number | null;
  catalog: Catalog;
  open: boolean;
  onToggleOpen: () => void;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
}

function EffectCard({ tracks, effect, trackId, catalog, open, onToggleOpen, onCommand, onEndGesture }: EffectCardProps) {
  const plugins = useContext(PluginEffects);
  const builtIn = catalog.effects.find((d) => d.kind === effect.kind);
  const description = effect.plugin ? { name: effect.plugin.name, params: [] } : builtIn;
  return (
    <div className={`effect${effect.enabled ? "" : " bypassed"}`}>
      <div className="effect-header">
        <button
          className={effect.enabled ? "tiny on power" : "tiny power"}
          aria-pressed={effect.enabled}
          title={effect.enabled ? "Bypass" : "Turn on"}
          onClick={() =>
            void onCommand({
              command: "set_effect_enabled",
              track_id: trackId,
              effect_id: effect.id,
              enabled: !effect.enabled,
            }).then(onEndGesture)
          }
        >
          ⏻
        </button>
        <button className="effect-name" onClick={onToggleOpen} aria-expanded={open}>
          {open ? "▾" : "▸"} {description?.name ?? effect.kind}
        </button>
        <button
          className="tiny delete"
          aria-label={`Remove ${description?.name ?? effect.kind}`}
          onClick={() =>
            void onCommand({ command: "remove_effect", track_id: trackId, effect_id: effect.id }).then(onEndGesture)
          }
        >
          ✕
        </button>
      </div>
      {open && effect.plugin && (
        <div className="effect-params">
          <button
            className="small"
            disabled={!plugins}
            onClick={() => plugins?.openWindow(effect.id)}
            title={plugins ? "Show the plugin's own controls" : "Plugins need the desktop app"}
          >
            Open plugin window
          </button>
          <span className="muted hint">{effect.plugin.vendor}</span>
        </div>
      )}
      {open && effect.kind === "compressor" && (
        <label className="sidechain" title="Duck this track whenever another one plays, e.g. the bass under the kick">
          <span>Listens to</span>
          <select
            aria-label={`${description?.name ?? effect.kind} listens to`}
            value={effect.sidechain ?? ""}
            onChange={(e) => {
              void onCommand({
                command: "set_effect_sidechain",
                track_id: trackId,
                effect_id: effect.id,
                source: e.target.value === "" ? null : Number(e.target.value),
              }).then(onEndGesture);
              e.currentTarget.blur();
            }}
          >
            <option value="">Its own sound</option>
            {tracks
              .filter((t) => t.id !== trackId)
              .map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name} (sidechain)
                </option>
              ))}
          </select>
        </label>
      )}
      {open && description && (
        <div className="effect-params">
          {description.params.map((spec) => (
            <ParamControl
              key={spec.id}
              spec={spec}
              value={effect.params[spec.id] ?? spec.default}
              onChange={(value) =>
                void onCommand({
                  command: "set_effect_param",
                  track_id: trackId,
                  effect_id: effect.id,
                  param: spec.id,
                  value,
                })
              }
              onCommit={onEndGesture}
            />
          ))}
        </div>
      )}
    </div>
  );
}
