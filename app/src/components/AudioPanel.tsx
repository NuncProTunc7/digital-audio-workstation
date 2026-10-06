import type { Clip, Command, InputStatus, Peaks, Track } from "../types";
import { meterPercent } from "../format";

const DEFAULT_INPUT = "__default__";
/** Normalize brings the loudest moment to this level. */
const NORMALIZE_TARGET_DB = -1;
const MIN_GAIN_DB = -30;
const MAX_GAIN_DB = 24;

interface AudioPanelProps {
  track: Track;
  /** The selected clip, if it is on this track. */
  clip: Clip | undefined;
  peaks: Peaks | undefined;
  missing: boolean;
  tempoBpm: number;
  playheadBeats: number;
  recording: boolean;
  input: InputStatus | null;
  onInputDevice: (name: string | null) => void;
  onCommand: (command: Command) => Promise<unknown>;
  onEndGesture: () => void;
  onImport: () => void;
  onRecord: () => void;
}

function formatSeconds(s: number): string {
  return s < 1 ? `${Math.round(s * 1000)} ms` : `${s.toFixed(2)} s`;
}

/** The dock for an audio track: microphone, recording, and clip settings. */
export default function AudioPanel(props: AudioPanelProps) {
  const { track, clip, input } = props;
  const audio = clip?.audio ?? undefined;
  // Real playing time of the clip (stretched clips play at the song tempo).
  const clipSeconds = clip ? (clip.length_beats * 60) / props.tempoBpm : 0;
  const maxFade = Math.max(0.01, Math.min(10, clipSeconds / 2));
  const insideClip =
    clip !== undefined &&
    props.playheadBeats > clip.start_beats + 1e-6 &&
    props.playheadBeats < clip.start_beats + clip.length_beats - 1e-6;

  const setAudio = (fields: Partial<Record<"gain_db" | "fade_in_seconds" | "fade_out_seconds", number>>) => {
    if (!clip) return;
    void props.onCommand({
      command: "set_audio_clip",
      clip_id: clip.id,
      gain_db: fields.gain_db ?? null,
      fade_in_seconds: fields.fade_in_seconds ?? null,
      fade_out_seconds: fields.fade_out_seconds ?? null,
    });
  };

  return (
    <section className="audio-panel" aria-label={`${track.name} audio`}>
      <header className="instrument-header">
        <h2>{track.name}</h2>
        <span className="instrument-kind">Audio</span>
        <span className="spacer" />
        <button onClick={props.onImport} title="WAV, MP3, M4A (phone recordings), FLAC or OGG">
          Import audio…
        </button>
      </header>

      <div className="audio-sections">
        <section className="param-group audio-input">
          <h3>Microphone</h3>
          <label className="field">
            <span>Input</span>
            <select
              aria-label="Audio input"
              value={input?.active ?? DEFAULT_INPUT}
              onChange={(e) => {
                props.onInputDevice(e.target.value === DEFAULT_INPUT ? null : e.target.value);
                e.currentTarget.blur();
              }}
            >
              <option value={DEFAULT_INPUT}>
                System default{input?.default_device ? ` (${input.default_device})` : ""}
              </option>
              {(input?.devices ?? []).map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </label>
          <div className="input-meter" title="Microphone level" aria-label="Microphone level">
            <div
              className={(input?.level ?? 0) > 0.9 ? "meter-fill hot" : "meter-fill"}
              style={{ width: `${meterPercent(input?.level ?? 0)}%` }}
            />
          </div>
          {input?.error && (
            <p className="error" role="alert">
              {input.error}
            </p>
          )}
          <button className={props.recording ? "record recording" : "record"} onClick={props.onRecord}>
            {props.recording ? "■ Stop recording" : "● Record"}
          </button>
          <p className="muted hint">
            Press <kbd>R</kbd> to record from the playhead and again to stop. Use headphones so the mic doesn't pick up
            the song. Too quiet? Move closer; the meter should bounce well past halfway without turning red.
          </p>
        </section>

        <section className="param-group audio-clip">
          <h3>Clip</h3>
          {clip && audio ? (
            <>
              <p className="clip-title">
                <strong>{clip.name}</strong>{" "}
                <span className="muted">
                  {formatSeconds(clipSeconds)} of {formatSeconds(audio.file_seconds)}
                </span>
              </p>
              {props.missing && (
                <p className="error" role="alert">
                  The audio file {audio.file} is missing from the project's audio folder.
                </p>
              )}
              <label className="param">
                <span>Gain</span>
                <input
                  type="range"
                  aria-label="Clip gain"
                  min={MIN_GAIN_DB}
                  max={MAX_GAIN_DB}
                  step={0.5}
                  value={Math.max(MIN_GAIN_DB, audio.gain_db)}
                  onChange={(e) => setAudio({ gain_db: Number(e.target.value) })}
                  onPointerUp={props.onEndGesture}
                  onKeyUp={props.onEndGesture}
                />
                <span className="param-value">
                  {audio.gain_db > 0 ? "+" : ""}
                  {audio.gain_db.toFixed(1)} dB
                </span>
              </label>
              <label className="param">
                <span>Fade in</span>
                <input
                  type="range"
                  aria-label="Fade in"
                  min={0}
                  max={maxFade}
                  step={0.01}
                  value={Math.min(maxFade, audio.fade_in_seconds)}
                  onChange={(e) => setAudio({ fade_in_seconds: Number(e.target.value) })}
                  onPointerUp={props.onEndGesture}
                  onKeyUp={props.onEndGesture}
                />
                <span className="param-value">{formatSeconds(audio.fade_in_seconds)}</span>
              </label>
              <label className="param">
                <span>Fade out</span>
                <input
                  type="range"
                  aria-label="Fade out"
                  min={0}
                  max={maxFade}
                  step={0.01}
                  value={Math.min(maxFade, audio.fade_out_seconds)}
                  onChange={(e) => setAudio({ fade_out_seconds: Number(e.target.value) })}
                  onPointerUp={props.onEndGesture}
                  onKeyUp={props.onEndGesture}
                />
                <span className="param-value">{formatSeconds(audio.fade_out_seconds)}</span>
              </label>
              <label className="dialog-check" title="Stretch the clip (keeping its pitch) when you change the song's tempo">
                <input
                  type="checkbox"
                  aria-label="Follow song tempo"
                  checked={audio.source_bpm != null}
                  onChange={(e) => {
                    void props
                      .onCommand({
                        command: "set_clip_tempo",
                        clip_id: clip.id,
                        source_bpm: e.target.checked ? props.tempoBpm : null,
                      })
                      .then(props.onEndGesture);
                  }}
                />
                Follow song tempo
                {audio.source_bpm != null && <span className="muted">(recorded at {audio.source_bpm} BPM)</span>}
              </label>
              <div className="button-row">
                <button
                  disabled={!props.peaks || props.peaks.peak <= 0}
                  onClick={() => {
                    const peak = props.peaks?.peak ?? 0;
                    if (peak <= 0) return;
                    const gain = Math.max(-60, Math.min(MAX_GAIN_DB, NORMALIZE_TARGET_DB - 20 * Math.log10(peak)));
                    setAudio({ gain_db: Math.round(gain * 10) / 10 });
                    props.onEndGesture();
                  }}
                  title="Set the gain so the loudest moment reaches -1 dB"
                >
                  Normalize
                </button>
                <button
                  disabled={!insideClip}
                  onClick={() =>
                    void props
                      .onCommand({ command: "split_clip", clip_id: clip.id, at_beats: props.playheadBeats })
                      .then(props.onEndGesture)
                  }
                  title="Cut the clip in two at the playhead (Ctrl+E)"
                >
                  Split at playhead
                </button>
              </div>
            </>
          ) : (
            <p className="muted">
              Click an audio clip to change its volume and fades. Drag a clip's edges on the timeline to trim it, or drop
              audio files from your phone onto the timeline.
            </p>
          )}
        </section>
      </div>
    </section>
  );
}
