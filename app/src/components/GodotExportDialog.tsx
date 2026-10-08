import { useState } from "react";
import type { ExportReport, GodotOptions, Project } from "../types";

const STORAGE_KEY = "npt.godotProjectDir";
const LOUDNESS = [
  { label: "-14 LUFS (loud, like streaming)", value: -14 },
  { label: "-16 LUFS (game music, recommended)", value: -16 },
  { label: "-18 LUFS", value: -18 },
  { label: "-20 LUFS (quiet background)", value: -20 },
];

function rememberedDir(): string {
  try {
    return window.localStorage.getItem(STORAGE_KEY) ?? "";
  } catch {
    return "";
  }
}

function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "music";
}

interface GodotExportDialogProps {
  project: Project;
  onPickFolder: () => Promise<string | null>;
  onExport: (options: GodotOptions) => Promise<ExportReport | undefined>;
  onClose: () => void;
}

/** Export loops (and stems) straight into a Godot project folder. */
export default function GodotExportDialog({ project, onPickFolder, onExport, onClose }: GodotExportDialogProps) {
  const [dir, setDir] = useState(rememberedDir);
  const [folder, setFolder] = useState("music");
  const [name, setName] = useState(slug(project.name));
  const [format, setFormat] = useState<"ogg" | "wav">("ogg");
  const [region, setRegion] = useState<"loop" | "song">(project.loop_region.enabled ? "loop" : "song");
  const [looped, setLooped] = useState(true);
  const [intro, setIntro] = useState(false);
  const sectionCount = project.markers?.length ?? 0;
  const [markerSections, setMarkerSections] = useState(sectionCount > 0);
  const [stems, setStems] = useState(false);
  const [lufs, setLufs] = useState<number | null>(-16);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ExportReport | null>(null);

  const loop = project.loop_region;
  // An intro needs a loop that starts after the song start.
  const introPossible = region === "loop" && looped && loop.enabled && loop.start_beats > 0;
  const run = async () => {
    setBusy(true);
    setReport(null);
    try {
      window.localStorage.setItem(STORAGE_KEY, dir);
    } catch {
      // Remembering the folder is a convenience only.
    }
    const useLoop = region === "loop";
    const result = await onExport({
      project_dir: dir,
      folder,
      name,
      format,
      start_beats: useLoop ? loop.start_beats : 0,
      end_beats: useLoop ? loop.end_beats : null,
      looped,
      intro: introPossible && intro,
      sections_from_markers: sectionCount > 0 && markerSections,
      stems,
      layers: stems,
      target_lufs: lufs,
      normalize: lufs !== null,
    });
    setBusy(false);
    if (result) setReport(result);
  };

  return (
    <div className="dialog-backdrop" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-label="Export to Godot">
        <div className="claude-panel-header">
          <strong>Export to Godot</strong>
          <span className="spacer" />
          <button className="small" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>

        <label className="dialog-field">
          <span>Godot project</span>
          <div className="dialog-row">
            <input
              aria-label="Godot project folder"
              placeholder="The folder with project.godot"
              value={dir}
              onChange={(e) => setDir(e.target.value)}
            />
            <button onClick={() => void onPickFolder().then((d) => d && setDir(d))}>Browse…</button>
          </div>
        </label>
        <div className="dialog-grid">
          <label className="dialog-field">
            <span>Folder in the project</span>
            <input aria-label="Folder in the project" value={folder} onChange={(e) => setFolder(e.target.value)} />
          </label>
          <label className="dialog-field">
            <span>File name</span>
            <input aria-label="File name" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="dialog-field">
            <span>What</span>
            <select aria-label="What to export" value={region} onChange={(e) => setRegion(e.target.value as "loop" | "song")}>
              <option value="loop" disabled={!loop.enabled}>
                Loop region (bars {loop.start_beats / project.time_signature.numerator + 1}–
                {loop.end_beats / project.time_signature.numerator})
              </option>
              <option value="song">Whole song</option>
            </select>
          </label>
          <label className="dialog-field">
            <span>Format</span>
            <select aria-label="Format" value={format} onChange={(e) => setFormat(e.target.value as "ogg" | "wav")}>
              <option value="ogg">OGG (small, recommended)</option>
              <option value="wav">WAV</option>
            </select>
          </label>
          <label className="dialog-field">
            <span>Loudness</span>
            <select
              aria-label="Loudness"
              value={lufs ?? "off"}
              onChange={(e) => setLufs(e.target.value === "off" ? null : Number(e.target.value))}
            >
              {LOUDNESS.map((l) => (
                <option key={l.value} value={l.value}>
                  {l.label}
                </option>
              ))}
              <option value="off">Keep the mix level</option>
            </select>
          </label>
        </div>
        <label className="dialog-check">
          <input type="checkbox" checked={looped} onChange={(e) => setLooped(e.target.checked)} />
          Seamless loop (the end flows into the start, and Godot loops it)
        </label>
        <label
          className="dialog-check"
          title={introPossible ? undefined : "Needs the loop region on, starting after bar 1, and Seamless loop"}
        >
          <input
            type="checkbox"
            checked={introPossible && intro}
            disabled={!introPossible}
            onChange={(e) => setIntro(e.target.checked)}
          />
          Play from the song start, then loop the loop region (what comes before it is an intro that plays once)
        </label>
        <label
          className="dialog-check"
          title={sectionCount > 0 ? undefined : "Double-click the strip under the ruler to mark sections first"}
        >
          <input
            type="checkbox"
            checked={sectionCount > 0 && markerSections}
            disabled={sectionCount === 0}
            onChange={(e) => setMarkerSections(e.target.checked)}
          />
          Sections from markers{sectionCount > 0 ? ` (${(project.markers ?? []).map((m) => m.name).join(", ")})` : ""}: one
          loop per section, plus an interactive resource that switches between them on the next bar
        </label>
        <label className="dialog-check">
          <input type="checkbox" checked={stems} onChange={(e) => setStems(e.target.checked)} />
          Also export each track as a stem, with a layers resource for adaptive music
        </label>

        <div className="button-row dialog-actions">
          <button className="primary" disabled={!dir.trim() || busy} onClick={() => void run()}>
            {busy ? "Exporting…" : "Export"}
          </button>
        </div>

        {report && (
          <div className="dialog-result" role="status">
            <p>
              ✓ Exported {report.seconds.toFixed(1)} s
              {report.integrated_lufs !== null ? ` at ${report.integrated_lufs.toFixed(1)} LUFS` : ""}
              {report.loop_start_seconds ? `, looping from ${report.loop_start_seconds.toFixed(2)} s` : ""}. In Godot, drag{" "}
              <code>{report.files[0]}</code> onto an AudioStreamPlayer.
            </p>
            <ul>
              {report.files.map((f) => (
                <li key={f}>
                  <code>{f}</code>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
