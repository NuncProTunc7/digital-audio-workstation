import { useState } from "react";
import { formatDb, meterPercent } from "../format";
import type { Catalog, Command, Effect, EffectKind, Project } from "../types";
import Fader from "./Fader";
import ParamControl from "./ParamControl";

interface MixerProps {
  project: Project;
  catalog: Catalog;
  trackPeaks: number[];
  masterPeaks: [number, number];
  selectedTrackId: number;
  onSelectTrack: (id: number) => void;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
}

/** Channel strips for every track plus the master bus. */
export default function Mixer(props: MixerProps) {
  const { project } = props;
  const send = (command: Command) => void props.onCommand(command);
  return (
    <div className="mixer">
      {project.tracks.map((t, i) => (
        <Strip
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
        </Strip>
      ))}
      <Strip
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
  name: string;
  master?: boolean;
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
  return (
    <section
      className={`strip${props.master ? " master" : ""}${props.selected ? " selected" : ""}`}
      aria-label={`${props.name} channel`}
      onPointerDown={props.onSelect}
    >
      <header className="strip-name">{props.name}</header>

      <div className="effects">
        {props.effects.map((e) => (
          <EffectCard
            key={e.id}
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
  effect: Effect;
  trackId: number | null;
  catalog: Catalog;
  open: boolean;
  onToggleOpen: () => void;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
}

function EffectCard({ effect, trackId, catalog, open, onToggleOpen, onCommand, onEndGesture }: EffectCardProps) {
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
