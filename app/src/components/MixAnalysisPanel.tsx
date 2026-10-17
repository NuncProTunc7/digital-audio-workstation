import { useState } from "react";
import {
  bandLabel,
  bandRange,
  barTicks,
  loudnessNote,
  loudnessVerdict,
  peakNote,
  peakVerdict,
  stereoNote,
  stereoVerdict,
  type Verdict,
} from "../mixAnalysis";
import type { MixAnalysis, Project } from "../types";

interface MixAnalysisPanelProps {
  project: Project;
  /** The last analysis, if any. */
  analysis: MixAnalysis | null;
  /** The song has changed since `analysis` was made. */
  stale: boolean;
  busy: boolean;
  /** Analyzes start..end beats (null: the whole song, with its reverb tail). */
  onAnalyze: (startBeats: number | null, endBeats: number | null) => void;
  selectedTrackId: number;
  onSelectTrack: (trackId: number) => void;
}

const VERDICT_ICON: Record<Verdict, string> = { good: "✓", check: "⚠", bad: "✗" };

/** Spectrogram frequency guides (the picture runs 30 Hz to 20 kHz, log scale). */
const FREQ_GUIDES = [
  { hz: 100, label: "100 Hz" },
  { hz: 1000, label: "1 kHz" },
  { hz: 10000, label: "10 kHz" },
];
const freqFromBottom = (hz: number) => Math.log(hz / 30) / Math.log(20000 / 30);

function db(v: number | null, unit: string): string {
  return v === null || v <= -120 ? "silent" : `${v.toFixed(1)} ${unit}`;
}

function Card({ title, value, note, verdict, hint }: { title: string; value: string; note: string; verdict: Verdict; hint: string }) {
  return (
    <div className={`analysis-card ${verdict}`} title={hint}>
      <div className="analysis-card-title">
        <span aria-hidden>{VERDICT_ICON[verdict]}</span> {title}
      </div>
      <div className="analysis-card-value">{value}</div>
      <div className="analysis-card-note">{note}</div>
    </div>
  );
}

/** What the song sounds like in numbers and pictures: the AI's analyze_mix, on screen. */
export default function MixAnalysisPanel({
  project,
  analysis,
  stale,
  busy,
  onAnalyze,
  selectedTrackId,
  onSelectTrack,
}: MixAnalysisPanelProps) {
  const loop = project.loop_region;
  const [range, setRange] = useState<"song" | "loop">("song");
  const useLoop = range === "loop" && loop.enabled;
  const [inWords, setInWords] = useState(false);
  const mix = analysis?.mix;
  const ticks = analysis ? barTicks(project, analysis) : [];
  const loudestShare = Math.max(1, ...(analysis?.tracks ?? []).map((t) => t.energy_share_percent));
  const loudestBand = Math.max(1, ...(mix?.bands ?? []).map((b) => b.share_percent));

  return (
    <div className="mix-analysis">
      <div className="analysis-toolbar">
        <select aria-label="What to analyze" value={useLoop ? "loop" : "song"} onChange={(e) => setRange(e.target.value as "song" | "loop")}>
          <option value="song">Whole song</option>
          <option value="loop" disabled={!loop.enabled}>
            Loop region{loop.enabled ? "" : " (turn Loop on)"}
          </option>
        </select>
        <button
          className="primary"
          disabled={busy}
          onClick={() => (useLoop ? onAnalyze(loop.start_beats, loop.end_beats) : onAnalyze(null, null))}
          title="Renders the song (and each track on its own) and measures it, the same way your AI checks a mix"
        >
          {busy ? "Analyzing…" : analysis ? "Analyze again" : "Analyze"}
        </button>
        {analysis && stale && !busy && <span className="analysis-stale">The song has changed since. Analyze again to update.</span>}
        {!analysis && !busy && (
          <span className="muted">See how loud, how wide and how bright your mix is, against the usual targets for game music.</span>
        )}
      </div>

      {analysis && mix && (
        <div className={`analysis-body${stale ? " stale" : ""}`}>
          <div className="analysis-cards">
            <Card
              title="Loudness"
              value={db(mix.integrated_lufs, "LUFS")}
              note={`${loudnessNote(mix.integrated_lufs)}${mix.max_short_term_lufs !== null ? ` · loudest moment ${mix.max_short_term_lufs.toFixed(1)}` : ""}`}
              verdict={loudnessVerdict(mix.integrated_lufs)}
              hint="Average loudness over the whole range (LUFS). Game music usually sits between -20 and -16."
            />
            <Card
              title="Peaks"
              value={db(mix.true_peak_dbtp, "dB")}
              note={peakNote(mix.true_peak_dbtp)}
              verdict={mix.integrated_lufs === null ? "check" : peakVerdict(mix.true_peak_dbtp)}
              hint="The loudest instant (true peak, dBTP). Keep it at -1 or below so exported files don't distort."
            />
            <Card
              title="Stereo width"
              value={mix.integrated_lufs === null ? "—" : mix.stereo_correlation.toFixed(2)}
              note={mix.integrated_lufs === null ? "Silent" : stereoNote(mix.stereo_correlation)}
              verdict={mix.integrated_lufs === null ? "check" : stereoVerdict(mix.stereo_correlation)}
              hint="How alike the left and right sides are: 1 is mono, around 0.3-0.9 sounds wide, below 0 parts can cancel out on a phone speaker."
            />
          </div>

          <section className="analysis-section">
            <h4>What to fix</h4>
            <ul className="analysis-hints" aria-label="Hints">
              {analysis.hints.map((h) => (
                <li key={h}>{h}</li>
              ))}
            </ul>
          </section>

          <div className="analysis-columns">
            <section className="analysis-section" aria-label="Tone">
              <h4 title="How the mix's energy is shared between low and high sounds. Most music carries the most in the bass.">Tone</h4>
              {mix.bands.map((b) => (
                <div key={b.name} className="analysis-bar-row" title={bandRange(b.from_hz, b.to_hz)}>
                  <span className="analysis-bar-label">{bandLabel(b.name)}</span>
                  <span className="analysis-bar">
                    <span className="analysis-bar-fill tone" style={{ width: `${(100 * b.share_percent) / loudestBand}%` }} />
                  </span>
                  <span className="analysis-bar-value">{b.share_percent.toFixed(0)}%</span>
                </div>
              ))}
            </section>

            {analysis.tracks.length > 0 && (
              <section className="analysis-section" aria-label="Tracks">
                <h4 title="Each track's share of the mix's energy (measured on its own). Click one to select it.">Tracks</h4>
                {analysis.tracks.map((t) => (
                  <button
                    key={t.id}
                    className={`analysis-bar-row track${t.id === selectedTrackId ? " selected" : ""}`}
                    onClick={() => onSelectTrack(t.id)}
                    title={t.lufs === null ? "Silent in this range" : `${t.lufs.toFixed(1)} LUFS on its own, peaks ${t.peak_dbfs.toFixed(1)} dB`}
                  >
                    <span className="analysis-bar-label">{t.name}</span>
                    <span className="analysis-bar">
                      <span className="analysis-bar-fill" style={{ width: `${(100 * t.energy_share_percent) / loudestShare}%` }} />
                    </span>
                    <span className="analysis-bar-value">{t.lufs === null ? "silent" : `${t.energy_share_percent.toFixed(0)}%`}</span>
                  </button>
                ))}
              </section>
            )}
          </div>

          <section className="analysis-section">
            <h4 title="Time runs left to right, low sounds at the bottom, high at the top; brighter means louder">Spectrogram</h4>
            {analysis.spectrogram_png_base64 ? (
              <figure className="analysis-spectrogram">
                <div className="analysis-spectrogram-picture">
                  <img src={`data:image/png;base64,${analysis.spectrogram_png_base64}`} alt="Spectrogram of the mix" />
                  {FREQ_GUIDES.map((g) => (
                    <span key={g.hz} className="analysis-freq" style={{ bottom: `${100 * freqFromBottom(g.hz)}%` }}>
                      {g.label}
                    </span>
                  ))}
                </div>
                <div className="analysis-time" aria-label="Bars">
                  {ticks.map((t) => (
                    <span key={t.bar} style={{ left: `${100 * t.at}%` }}>
                      {t.bar}
                    </span>
                  ))}
                </div>
              </figure>
            ) : (
              <p className="muted">No picture this time.</p>
            )}
            {(analysis.spectrogram_summary ?? []).length > 0 && (
              <>
                <button className="small" aria-expanded={inWords} onClick={() => setInWords((v) => !v)}>
                  {inWords ? "In words ▴" : "In words ▾"}
                </button>
                {inWords && (
                  <ul className="analysis-words">
                    {(analysis.spectrogram_summary ?? []).map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
