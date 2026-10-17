import type { Catalog, Command, Effect, EffectKind, Track } from "../types";
import ParamControl from "./ParamControl";

interface PedalboardProps {
  track: Track;
  catalog: Catalog;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
}

/**
 * A track's effect chain drawn as a row of stomp boxes, left to right in
 * the order the sound goes through them, with one-click rigs. Every change
 * is the same Command the Mixer's effect cards use.
 */
export default function Pedalboard({ track, catalog, onCommand, onEndGesture }: PedalboardProps) {
  const effects = track.mixer.effects;
  const pedalKinds = catalog.effects.filter((d) => isPedal(d.kind));
  const studioKinds = catalog.effects.filter((d) => !isPedal(d.kind));
  const send = (c: Command) => void onCommand(c).then(onEndGesture);

  const move = (e: Effect, by: number) => {
    const from = effects.findIndex((x) => x.id === e.id);
    const to = from + by;
    if (from < 0 || to < 0 || to >= effects.length) return;
    send({
      command: "batch",
      commands: [
        { command: "remove_effect", track_id: track.id, effect_id: e.id },
        { command: "restore_effect", track_id: track.id, index: to, effect: e },
      ],
    });
  };

  return (
    <section className="pedalboard" aria-label="Pedalboard">
      <header className="pedalboard-header">
        <strong>Pedalboard</strong>
        <select
          aria-label="Rig"
          value=""
          title="Replace the pedals with a ready-made setup (undo brings yours back)"
          onChange={(e) => {
            const rig = e.target.value;
            e.currentTarget.blur();
            if (rig) send({ command: "load_rig", track_id: track.id, rig });
          }}
        >
          <option value="">Rig ▾</option>
          {catalog.rigs.map((r) => (
            <option key={r.name} value={r.name} title={r.description}>
              {r.name}
            </option>
          ))}
        </select>
        <select
          aria-label="Add a pedal"
          value=""
          onChange={(e) => {
            const kind = e.target.value as EffectKind;
            e.currentTarget.blur();
            if (kind) send({ command: "add_effect", track_id: track.id, kind, index: null });
          }}
        >
          <option value="">+ Pedal</option>
          <optgroup label="Pedals">
            {pedalKinds.map((d) => (
              <option key={d.kind} value={d.kind} title={d.description}>
                {d.name}
              </option>
            ))}
          </optgroup>
          <optgroup label="Studio">
            {studioKinds.map((d) => (
              <option key={d.kind} value={d.kind} title={d.description}>
                {d.name}
              </option>
            ))}
          </optgroup>
        </select>
        <span className="muted">The sound goes left to right. These are the same effects as in the Mixer.</span>
      </header>
      {effects.length === 0 ? (
        <p className="muted pedalboard-empty">No pedals yet. Pick a Rig to start from a ready-made sound, or add pedals one by one.</p>
      ) : (
        <div className="pedals">
          {effects.map((e, i) => {
            const d = catalog.effects.find((x) => x.kind === e.kind);
            const name = e.plugin?.name ?? d?.name ?? e.kind;
            return (
              <div
                key={e.id}
                role="group"
                aria-label={`${name} pedal`}
                className={`pedal pedal-${e.kind}${e.enabled ? "" : " off"}`}
                title={d?.description}
              >
                <div className="pedal-top">
                  <button
                    className="tiny"
                    aria-label={`Move ${name} earlier`}
                    disabled={i === 0}
                    onClick={() => move(e, -1)}
                  >
                    ◀
                  </button>
                  <span className="pedal-name">{name}</span>
                  <button
                    className="tiny"
                    aria-label={`Move ${name} later`}
                    disabled={i === effects.length - 1}
                    onClick={() => move(e, 1)}
                  >
                    ▶
                  </button>
                  <button
                    className="tiny delete"
                    aria-label={`Remove ${name}`}
                    onClick={() => send({ command: "remove_effect", track_id: track.id, effect_id: e.id })}
                  >
                    ✕
                  </button>
                </div>
                <div className="pedal-knobs">
                  {(d?.params ?? []).map((spec) => (
                    <ParamControl
                      key={spec.id}
                      spec={spec}
                      value={e.params[spec.id] ?? spec.default}
                      onChange={(value) =>
                        void onCommand({
                          command: "set_effect_param",
                          track_id: track.id,
                          effect_id: e.id,
                          param: spec.id,
                          value,
                        })
                      }
                      onCommit={onEndGesture}
                    />
                  ))}
                  {e.plugin && <span className="muted">Plugin: open it from the Mixer.</span>}
                </div>
                <button
                  className={`footswitch${e.enabled ? " on" : ""}`}
                  aria-label={`${name} ${e.enabled ? "on" : "off"}`}
                  aria-pressed={e.enabled}
                  title={e.enabled ? "Stomp to bypass" : "Stomp to turn on"}
                  onClick={() =>
                    send({ command: "set_effect_enabled", track_id: track.id, effect_id: e.id, enabled: !e.enabled })
                  }
                >
                  <span className="led" aria-hidden />
                </button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

const PEDALS = new Set<EffectKind>(["overdrive", "fuzz", "amp", "phaser", "flanger", "tremolo", "wah", "noise_gate", "octaver"]);

/** Guitar pedals, listed first on + Pedal (as `EffectKind::is_pedal`). */
function isPedal(kind: EffectKind): boolean {
  return PEDALS.has(kind);
}
