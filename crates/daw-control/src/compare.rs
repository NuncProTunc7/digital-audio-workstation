//! A/B listening: switching between the song and a saved version while it
//! plays, at matched loudness, so the louder one doesn't win just for being
//! louder.

use std::sync::Arc;

use daw_engine::AudioPool;
use daw_model::{Id, MIN_VOLUME_DB, Project};
use serde::{Deserialize, Serialize};

/// Which version is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The song as it is now.
    Current,
    /// The saved version.
    Version,
}

/// The two versions' loudness and the level change that matches them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    pub snapshot_id: Id,
    pub name: String,
    /// Integrated loudness (LUFS) of each; None when silent.
    pub current_lufs: Option<f64>,
    pub version_lufs: Option<f64>,
    /// Applied while listening to each side (0 or negative: the louder one
    /// is turned down, never the quieter one up, so nothing clips).
    pub current_gain_db: f64,
    pub version_gain_db: f64,
}

/// The level changes that bring two loudness readings together by turning
/// the louder one down: `(current_gain_db, version_gain_db)`.
pub fn matching_gains(current_lufs: Option<f64>, version_lufs: Option<f64>) -> (f64, f64) {
    match (current_lufs, version_lufs) {
        (Some(a), Some(b)) if a > b => (b - a, 0.0),
        (Some(a), Some(b)) => (0.0, a - b),
        _ => (0.0, 0.0),
    }
}

fn loudness(project: &Project, audio: &Arc<AudioPool>) -> Option<f64> {
    let options = daw_analysis::AnalyzeOptions {
        per_track: false,
        ..Default::default()
    };
    daw_analysis::analyze(project, audio, &options)
        .mix
        .integrated_lufs
}

/// Renders the song and saved version `snapshot_id` offline and measures
/// both. Takes a few seconds for a long song; don't hold the session lock.
pub fn measure(
    project: &Project,
    audio: &Arc<AudioPool>,
    snapshot_id: Id,
) -> Result<Comparison, String> {
    let version = project
        .snapshot_song(snapshot_id)
        .ok_or_else(|| format!("there is no saved version with id {snapshot_id}"))?;
    let name = project
        .snapshots
        .iter()
        .find(|s| s.id == snapshot_id)
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let current_lufs = loudness(project, audio);
    let version_lufs = loudness(&version, audio);
    let (current_gain_db, version_gain_db) = matching_gains(current_lufs, version_lufs);
    Ok(Comparison {
        snapshot_id,
        name,
        current_lufs,
        version_lufs,
        current_gain_db,
        version_gain_db,
    })
}

/// What the engine plays for `side`: the song or the version, with its
/// matching level change on the master.
pub fn listening_project(
    project: &Project,
    comparison: &Comparison,
    side: Side,
) -> Option<Project> {
    let (mut p, gain) = match side {
        Side::Current => (project.clone(), comparison.current_gain_db),
        Side::Version => (
            project.snapshot_song(comparison.snapshot_id)?,
            comparison.version_gain_db,
        ),
    };
    p.master.volume_db = (p.master.volume_db + gain).max(MIN_VOLUME_DB);
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_model::{Command, NoteInput, Session};

    #[test]
    fn the_louder_version_is_turned_down() {
        assert_eq!(matching_gains(Some(-14.0), Some(-18.0)), (-4.0, 0.0));
        assert_eq!(matching_gains(Some(-20.0), Some(-17.5)), (0.0, -2.5));
        assert_eq!(matching_gains(None, Some(-17.5)), (0.0, 0.0));
    }

    #[test]
    fn a_quieter_version_is_matched_to_the_song() {
        let mut s = Session::default();
        s.execute(Command::CreateClip {
            track_id: 1,
            start_beats: 0.0,
            length_beats: 8.0,
            name: None,
            notes: (0..8)
                .map(|b| NoteInput {
                    chance: 100,
                    pitch: 60,
                    start_beats: f64::from(b),
                    length_beats: 0.9,
                    velocity: 110,
                    id: None,
                })
                .collect(),
        })
        .expect("clip");
        s.execute(Command::TakeSnapshot {
            name: "Quiet".into(),
        })
        .expect("take");
        let version = s.project().snapshots[0].id;
        // The song gets 6 dB louder than its saved version.
        s.execute(Command::SetMasterVolume { volume_db: 6.0 })
            .expect("louder");
        let audio = AudioPool::in_temp_dir();
        let c = measure(s.project(), &audio, version).expect("measure");
        let (a, b) = (c.current_lufs.expect("a"), c.version_lufs.expect("b"));
        assert!((a - b - 6.0).abs() < 0.5, "{a} vs {b}");
        assert!((c.current_gain_db + 6.0).abs() < 0.5 && c.version_gain_db == 0.0);
        // Listening to the song now plays it at the version's level.
        let current = listening_project(s.project(), &c, Side::Current).expect("a");
        assert!((current.master.volume_db - 0.0).abs() < 0.5);
        let b_side = listening_project(s.project(), &c, Side::Version).expect("b");
        assert_eq!(b_side.master.volume_db, 0.0);
        assert!(b_side.snapshots.is_empty());
        assert!(measure(s.project(), &audio, 9_999).is_err());
    }
}
