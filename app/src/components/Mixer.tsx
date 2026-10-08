import { useState } from "react";
import { formatDb, meterPercent } from "../format";
import type { Bus, Catalog, Command, Effect, EffectKind, Project, Track } from "../types";
import Fader from "./Fader";
import ParamControl from "./ParamControl";

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
  return (
    <div className="mixer">
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
            <span className="param-value">
              {t.mixer.pan === 0 ? "C" : `${Math.round(Math.abs(t.mixer.pan) * 100)}${t.mixer.pan < 0 ? "L" : "R"}`}
            </span>
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
  );
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
            const kind = e.target.value as EffectKind;
            e.currentTarget.blur();
            void props
              .onCommand({ command: "add_effect", track_id: props.trackId, kind, index: null })
              .then(props.onEndGesture);
          }}
        >
          <option value="">+ Add effect…</option>
          {props.catalog.effects.map((d) => (
            <option key={d.kind} value={d.kind} title={d.description}>
              {d.name}
            </option>
          ))}
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
      <div className="strip-db">{formatDb(props.volumeDb)}</div>
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
  const description = catalog.effects.find((d) => d.kind === effect.kind);
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
