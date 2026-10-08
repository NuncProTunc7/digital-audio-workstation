//! Free sampled instruments the app can fetch for the Sampler: real
//! pianos, strings, brass, woodwinds. Each comes straight from its
//! publisher's GitHub (the sfzinstruments project); nothing is
//! redistributed by Nunc Pro Tune. Packs are unpacked into
//! `<app data>/sample-library/<id>/`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::discovery::APP_ID;

/// One downloadable instrument.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LibraryPack {
    pub id: &'static str,
    pub name: &'static str,
    /// What it is, in plain words.
    pub description: &'static str,
    /// SPDX-style license name.
    pub license: &'static str,
    /// Who to credit when using it in a game (CC-BY needs it).
    pub credit: &'static str,
    /// Repository under github.com/sfzinstruments.
    #[serde(skip)]
    pub repo: &'static str,
    /// The instrument's playable variants: (name, .sfz path in the repo).
    pub programs: &'static [(&'static str, &'static str)],
    /// Approximate download size.
    pub megabytes: u32,
}

/// The instruments on offer. Licenses checked Oct 2026 (CC0 needs no
/// credit; CC-BY needs the credit line in the game's credits).
pub const LIBRARY: &[LibraryPack] = &[
    LibraryPack {
        id: "salamander-grand-piano",
        name: "Salamander Grand Piano",
        description: "Yamaha C5 concert grand, 16 velocity layers. The best free piano.",
        license: "CC-BY-3.0",
        credit: "Salamander Grand Piano by Alexander Holm (CC BY 3.0)",
        repo: "SalamanderGrandPiano",
        programs: &[("Grand piano", "Salamander Grand Piano V3.sfz")],
        megabytes: 750,
    },
    LibraryPack {
        id: "headroom-piano",
        name: "Headroom Piano",
        description: "Yamaha C3 grand, warm and intimate; a smaller download.",
        license: "CC-BY-4.0",
        credit: "Headroom Piano by Bengt Nilsson (CC BY 4.0)",
        repo: "BengtNilsson.HeadroomPiano",
        programs: &[
            ("Headroom piano", "Headroom Piano.sfz"),
            ("Intimate piano", "Intimate Piano.sfz"),
        ],
        megabytes: 140,
    },
    LibraryPack {
        id: "cello",
        name: "Cello",
        description: "Solo cello, bowed and plucked: melodies, sad themes, string parts.",
        license: "CC0-1.0",
        credit: "Cello by Karoryfer Samples and Bigcat Instruments (CC0)",
        repo: "karoryfer-bigcat.cello",
        programs: &[
            ("Bowed", "Programs/01- Bowed (velocity layer).sfz"),
            ("Plucked", "Programs/03- Plucked.sfz"),
        ],
        megabytes: 130,
    },
    LibraryPack {
        id: "double-bass",
        name: "Double bass",
        description: "Upright bass, bowed and plucked: orchestral lows and jazzy walking bass.",
        license: "CC0-1.0",
        credit: "Double bass by D. Smolken (CC0)",
        repo: "dsmolken.double-bass",
        programs: &[
            ("Bowed (arco)", "d_smolken_rubner_bass_arco.sfz"),
            ("Plucked (pizzicato)", "d_smolken_rubner_bass_pizz.sfz"),
        ],
        megabytes: 260,
    },
    LibraryPack {
        id: "flute",
        name: "Flute",
        description: "Concert flute: light, airy melodies for villages and forests.",
        license: "CC-BY-4.0",
        credit: "Flute by Xavier Hosxe / Ixox (CC BY 4.0)",
        repo: "Ixox.Flute",
        programs: &[("Flute", "Ixox Flute.sfz")],
        megabytes: 10,
    },
    LibraryPack {
        id: "war-tuba",
        name: "War Tuba",
        description: "Big, gritty tuba: heavy brass for bosses and comic marches.",
        license: "CC0-1.0",
        credit: "War Tuba by Karoryfer Samples (CC0)",
        repo: "karoryfer.war-tuba",
        programs: &[("Tuba", "Programs/2-solo-poly.sfz")],
        megabytes: 110,
    },
];

/// `%APPDATA%\io.github.nuncprotunc7.nuncprotune\sample-library` on Windows.
pub fn library_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
        .join("sample-library")
}

/// How a download is going.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Job {
    /// Bytes so far, and the total when the server says.
    Downloading { bytes: u64, total: Option<u64> },
    /// Download done; unzipping into the library folder.
    Unpacking,
    /// The last attempt failed (try again to retry).
    Failed { error: String },
}

/// A pack, with whether it's on this computer and how a download is going.
#[derive(Debug, Clone, Serialize)]
pub struct PackState {
    #[serde(flatten)]
    pub pack: LibraryPack,
    /// The playable programs (name, .sfz path), once downloaded.
    pub installed: Option<Vec<(String, String)>>,
    /// A download in progress, or the last one's failure.
    pub job: Option<Job>,
}

fn jobs() -> &'static Mutex<HashMap<&'static str, Job>> {
    static MAP: OnceLock<Mutex<HashMap<&'static str, Job>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_job(id: &'static str, job: Option<Job>) {
    if let Ok(mut m) = jobs().lock() {
        match job {
            Some(j) => m.insert(id, j),
            None => m.remove(id),
        };
    }
}

fn find(id: &str) -> Result<&'static LibraryPack, String> {
    LIBRARY
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("there is no instrument \"{id}\" in the library"))
}

/// Where an unpacked pack's programs are, if it is on this computer.
fn installed_programs(dir: &Path, pack: &LibraryPack) -> Option<Vec<(String, String)>> {
    let root = dir.join(pack.id).join(format!("{}-master", pack.repo));
    let programs: Vec<(String, String)> = pack
        .programs
        .iter()
        .map(|(name, sfz)| (name.to_string(), root.join(sfz)))
        .filter(|(_, p)| p.is_file())
        .map(|(n, p)| (n, p.display().to_string()))
        .collect();
    (programs.len() == pack.programs.len()).then_some(programs)
}

/// Every pack and its state.
pub fn list(dir: &Path) -> Vec<PackState> {
    let jobs = jobs().lock().map(|m| m.clone()).unwrap_or_default();
    LIBRARY
        .iter()
        .map(|p| PackState {
            pack: *p,
            installed: installed_programs(dir, p),
            job: jobs.get(p.id).cloned(),
        })
        .collect()
}

/// Starts downloading pack `id` in the background (watch it with
/// [`list`]). Does nothing if it is already downloading or installed.
pub fn start_download(dir: &Path, id: &str) -> Result<(), String> {
    let pack = find(id)?;
    if installed_programs(dir, pack).is_some() {
        return Ok(());
    }
    {
        let mut m = jobs().lock().map_err(|e| e.to_string())?;
        if matches!(
            m.get(pack.id),
            Some(Job::Downloading { .. } | Job::Unpacking)
        ) {
            return Ok(());
        }
        m.insert(
            pack.id,
            Job::Downloading {
                bytes: 0,
                total: None,
            },
        );
    }
    let dir = dir.to_path_buf();
    std::thread::Builder::new()
        .name(format!("download {}", pack.id))
        .spawn(move || {
            if let Err(e) = download(&dir, pack.id) {
                crate::diagnostics::log_error(&format!("Downloading {} failed: {e}", pack.name));
                set_job(pack.id, Some(Job::Failed { error: e }));
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Downloads and unpacks pack `id` into `dir` (blocks; run it off the UI
/// thread). Returns its programs' .sfz paths.
pub fn download(dir: &Path, id: &str) -> Result<Vec<(String, String)>, String> {
    let pack = find(id)?;
    if let Some(done) = installed_programs(dir, pack) {
        return Ok(done);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let url = format!(
        "https://codeload.github.com/sfzinstruments/{}/zip/refs/heads/master",
        pack.repo
    );
    let zip_path = dir.join(format!("{}.zip.part", pack.id));
    let result = fetch(&url, &zip_path, pack.id).and_then(|()| {
        set_job(pack.id, Some(Job::Unpacking));
        unpack(&zip_path, &dir.join(pack.id))
    });
    let _ = std::fs::remove_file(&zip_path);
    set_job(pack.id, None);
    result?;
    installed_programs(dir, pack).ok_or_else(|| {
        format!(
            "{} downloaded, but its instrument files weren't where expected",
            pack.name
        )
    })
}

fn fetch(url: &str, to: &Path, id: &'static str) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("couldn't download ({e}); check the internet connection"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let mut body = response.into_body().into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("the download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        set_job(id, Some(Job::Downloading { bytes: done, total }));
    }
    file.flush().map_err(|e| e.to_string())
}

/// Unpacks a zip into `to`, refusing entries that would land outside it.
fn unpack(zip_path: &Path, to: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("the download is damaged: {e}"))?;
    let tmp = to.with_extension("unpacking");
    let _ = std::fs::remove_dir_all(&tmp);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = tmp.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(to);
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())
}

/// Credits for the library instruments the song plays, for the game's
/// credits screen.
pub fn credits_for(project: &daw_model::Project, dir: &Path) -> Vec<&'static str> {
    LIBRARY
        .iter()
        .filter(|p| {
            let root = dir.join(p.id);
            project.tracks.iter().any(|t| {
                t.instrument
                    .sample_pack
                    .as_deref()
                    .is_some_and(|s| Path::new(s).starts_with(&root))
            })
        })
        .map(|p| p.credit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_are_found_once_unpacked() {
        let dir = tempfile::tempdir().expect("tmp");
        let pack = find("flute").expect("flute");
        assert!(installed_programs(dir.path(), pack).is_none());
        // A zip shaped like GitHub's, with the instrument file inside.
        let zip_path = dir.path().join("flute.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).expect("zip"));
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("Ixox.Flute-master/Ixox Flute.sfz", opts)
                .expect("entry");
            z.write_all(b"<region> sample=a.wav").expect("write");
            z.start_file("../escape.txt", opts).expect("entry");
            z.write_all(b"nope").expect("write");
            z.finish().expect("finish");
        }
        unpack(&zip_path, &dir.path().join("flute")).expect("unpack");
        let programs = installed_programs(dir.path(), pack).expect("installed");
        assert!(programs[0].1.ends_with("Ixox Flute.sfz"));
        assert!(
            !dir.path().join("escape.txt").exists(),
            "zip entries can't escape"
        );
        let states = list(dir.path());
        assert!(
            states
                .iter()
                .any(|s| s.pack.id == "flute" && s.installed.is_some())
        );
        // A song using it gets its credit line.
        let mut p = daw_model::Project::default();
        p.tracks[0].instrument.sample_pack = Some(programs[0].1.clone());
        assert_eq!(credits_for(&p, dir.path()), vec![pack.credit]);
        assert!(find("nothing").is_err());
    }

    /// Downloads the smallest pack for real: `cargo test -p daw-control
    /// library -- --ignored`.
    #[test]
    #[ignore = "downloads from GitHub"]
    fn downloads_a_real_pack() {
        let dir = tempfile::tempdir().expect("tmp");
        let programs = download(dir.path(), "flute").expect("download");
        let pack = daw_sampler::load_pack(Path::new(&programs[0].1), 512 << 20).expect("load");
        assert!(!pack.zones.is_empty());
    }
}
