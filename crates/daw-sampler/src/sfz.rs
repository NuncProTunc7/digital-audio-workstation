//! Reading SFZ files: the text format most free sample packs (including the
//! Salamander Grand Piano) use to map recordings onto keys and velocities.
//!
//! Supported: `<control>` `default_path`, `<global>`/`<master>`/`<group>`
//! inheritance, `<region>`s with `sample`, `key`, `lokey`, `hikey`,
//! `pitch_keycenter`, `lovel`, `hivel`, `volume`, `tune`, `transpose`,
//! `offset`, `ampeg_attack`, `ampeg_release`, `amp_veltrack`, `loop_mode`,
//! `loop_start`, `loop_end`, `trigger` (release-trigger regions are skipped),
//! note names (`c4` = 60), `#define`, `#include`, and comments.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::SamplerError;

/// One mapped recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub sample: PathBuf,
    pub lokey: u8,
    pub hikey: u8,
    pub keycenter: u8,
    pub lovel: u8,
    pub hivel: u8,
    pub volume_db: f32,
    /// Fine tuning plus transpose, in cents.
    pub tune_cents: f32,
    pub offset_frames: u64,
    pub attack_s: Option<f32>,
    pub release_s: Option<f32>,
    /// 0–100: how much velocity changes loudness.
    pub veltrack_percent: f32,
    /// Plays to the end whatever happens to the key.
    pub one_shot: bool,
    /// Loop points in frames (inclusive start, inclusive end, as in SFZ).
    pub loop_frames: Option<(u64, u64)>,
}

/// Parses a note number or name: `60`, `c4`, `C#4`, `eb3`.
pub fn parse_note(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i32>() {
        return u8::try_from(n.clamp(0, 127)).ok();
    }
    let mut chars = s.chars();
    let base = match chars.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (accidental, octave) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') if rest.len() > 1 => (-1, &rest[1..]),
        _ => (0, rest.as_str()),
    };
    let octave: i32 = octave.parse().ok()?;
    u8::try_from(((octave + 1) * 12 + base + accidental).clamp(0, 127)).ok()
}

/// Comments removed, `#define`s substituted, `#include`s inlined.
fn preprocess(
    text: &str,
    dir: &Path,
    defines: &mut Vec<(String, String)>,
    depth: u32,
) -> Result<String, SamplerError> {
    if depth > 8 {
        return Err(SamplerError::Parse("#include nests too deeply".into()));
    }
    // Block comments first, then line comments.
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("/*") {
        plain.push_str(&rest[..i]);
        rest = rest[i + 2..]
            .find("*/")
            .map_or("", |j| &rest[i + 2 + j + 2..]);
    }
    plain.push_str(rest);
    let mut out = String::with_capacity(plain.len());
    for line in plain.lines() {
        let line = line.find("//").map_or(line, |i| &line[..i]);
        let trimmed = line.trim();
        if let Some(def) = trimmed.strip_prefix("#define") {
            let mut parts = def.split_whitespace();
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                defines.push((k.to_owned(), v.to_owned()));
                // Longest names first so $KEY doesn't clobber $KEYCENTER.
                defines.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
            }
            continue;
        }
        if let Some(inc) = trimmed.strip_prefix("#include") {
            let name = inc.trim().trim_matches('"');
            let path = dir.join(name.replace('\\', "/"));
            let text = std::fs::read_to_string(&path).map_err(|e| SamplerError::Io {
                path: path.clone(),
                message: e.to_string(),
            })?;
            out.push_str(&preprocess(&text, dir, defines, depth + 1)?);
            out.push('\n');
            continue;
        }
        let mut l = line.to_owned();
        for (k, v) in defines.iter() {
            if l.contains(k.as_str()) {
                l = l.replace(k.as_str(), v);
            }
        }
        out.push_str(&l);
        out.push('\n');
    }
    Ok(out)
}

/// A header (`<region>`) or an opcode (`name=value`).
#[derive(Debug, PartialEq)]
enum Token {
    Header(String),
    Opcode(String, String),
}

/// Splits a line into headers and opcodes. Values may contain spaces
/// (sample paths often do): a value runs until the next `name=` or `<`.
fn tokens(line: &str) -> Vec<Token> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let is_name = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    // Where the next header or opcode starts, from `from`.
    let next_start = |from: usize| -> usize {
        let mut j = from;
        while j < bytes.len() {
            if bytes[j] == b'<' {
                return j;
            }
            let at_word = j == 0 || bytes[j - 1].is_ascii_whitespace();
            if at_word && is_name(bytes[j]) {
                let mut k = j;
                while k < bytes.len() && is_name(bytes[k]) {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'=' {
                    return j;
                }
            }
            j += 1;
        }
        bytes.len()
    };
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i] == b'<' {
            let end = line[i..].find('>').map_or(bytes.len(), |e| i + e);
            out.push(Token::Header(line[i + 1..end].trim().to_ascii_lowercase()));
            i = end + 1;
            continue;
        }
        let Some(eq) = line[i..].find('=').map(|e| i + e) else {
            break;
        };
        let name = line[i..eq].trim().to_ascii_lowercase();
        let end = next_start(eq + 1);
        let value = line[eq + 1..end].trim().to_owned();
        out.push(Token::Opcode(name, value));
        i = end;
    }
    out
}

type Opcodes = HashMap<String, String>;

fn num<T: std::str::FromStr>(ops: &Opcodes, key: &str) -> Option<T> {
    ops.get(key).and_then(|v| v.trim().parse().ok())
}

fn region_from(ops: &Opcodes, sample_dir: &Path) -> Option<Region> {
    if ops
        .get("trigger")
        .is_some_and(|t| t != "attack" && t != "first" && t != "legato")
    {
        return None;
    }
    let sample = ops.get("sample")?;
    let key = ops.get("key").and_then(|k| parse_note(k));
    let lokey = ops
        .get("lokey")
        .and_then(|k| parse_note(k))
        .or(key)
        .unwrap_or(0);
    let hikey = ops
        .get("hikey")
        .and_then(|k| parse_note(k))
        .or(key)
        .unwrap_or(127);
    let keycenter = ops
        .get("pitch_keycenter")
        .and_then(|k| parse_note(k))
        .or(key)
        .unwrap_or(60);
    let loop_mode = ops.get("loop_mode").map(String::as_str).unwrap_or("");
    let looping = matches!(loop_mode, "loop_continuous" | "loop_sustain");
    let loop_frames = match (
        looping,
        num::<u64>(ops, "loop_start"),
        num::<u64>(ops, "loop_end"),
    ) {
        (true, Some(s), Some(e)) if e > s => Some((s, e)),
        _ => None,
    };
    Some(Region {
        sample: sample_dir.join(sample.replace('\\', "/")),
        lokey: lokey.min(hikey),
        hikey: hikey.max(lokey),
        keycenter,
        lovel: num::<u8>(ops, "lovel").unwrap_or(1).clamp(1, 127),
        hivel: num::<u8>(ops, "hivel").unwrap_or(127).clamp(1, 127),
        volume_db: num(ops, "volume").unwrap_or(0.0),
        tune_cents: num::<f32>(ops, "tune").unwrap_or(0.0)
            + 100.0 * num::<f32>(ops, "transpose").unwrap_or(0.0),
        offset_frames: num(ops, "offset").unwrap_or(0),
        attack_s: num(ops, "ampeg_attack"),
        release_s: num(ops, "ampeg_release"),
        veltrack_percent: num::<f32>(ops, "amp_veltrack")
            .unwrap_or(100.0)
            .clamp(-100.0, 100.0),
        one_shot: loop_mode == "one_shot",
        loop_frames,
    })
}

/// Reads an SFZ file into its playable regions.
pub fn parse_sfz_file(path: &Path) -> Result<Vec<Region>, SamplerError> {
    let text = std::fs::read_to_string(path).map_err(|e| SamplerError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    parse_sfz(&text, dir)
}

/// Parses SFZ text; sample paths are relative to `dir`.
pub fn parse_sfz(text: &str, dir: &Path) -> Result<Vec<Region>, SamplerError> {
    let text = preprocess(text, dir, &mut Vec::new(), 0)?;
    let mut default_path = String::new();
    // Opcodes in effect at each level; a header resets its level and below.
    let (mut global, mut master, mut group) = (Opcodes::new(), Opcodes::new(), Opcodes::new());
    let mut region: Option<Opcodes> = None;
    let mut level = "";
    let mut out = Vec::new();
    let flush = |region: &mut Option<Opcodes>, out: &mut Vec<Region>, default_path: &str| {
        if let Some(ops) = region.take()
            && let Some(r) = region_from(&ops, &dir.join(default_path.replace('\\', "/")))
        {
            out.push(r);
        }
    };
    for line in text.lines() {
        for t in tokens(line) {
            match t {
                Token::Header(h) => {
                    flush(&mut region, &mut out, &default_path);
                    level = match h.as_str() {
                        "control" => "control",
                        "global" => {
                            global.clear();
                            master.clear();
                            group.clear();
                            "global"
                        }
                        "master" => {
                            master.clear();
                            group.clear();
                            "master"
                        }
                        "group" => {
                            group.clear();
                            "group"
                        }
                        "region" => {
                            let mut ops = global.clone();
                            ops.extend(master.clone());
                            ops.extend(group.clone());
                            region = Some(ops);
                            "region"
                        }
                        _ => "ignored",
                    };
                }
                Token::Opcode(k, v) => match level {
                    "control" if k == "default_path" => default_path = v,
                    "global" => {
                        global.insert(k, v);
                    }
                    "master" => {
                        master.insert(k, v);
                    }
                    "group" => {
                        group.insert(k, v);
                    }
                    "region" => {
                        if let Some(r) = region.as_mut() {
                            r.insert(k, v);
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    flush(&mut region, &mut out, &default_path);
    if out.is_empty() {
        return Err(SamplerError::Parse(
            "the SFZ file has no playable regions".into(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(parse_note("c4"), Some(60));
        assert_eq!(parse_note("C#4"), Some(61));
        assert_eq!(parse_note("eb3"), Some(51));
        assert_eq!(parse_note("a0"), Some(21));
        assert_eq!(parse_note("b-1"), Some(11));
        assert_eq!(parse_note("72"), Some(72));
        assert_eq!(parse_note("h4"), None);
    }

    #[test]
    fn inheritance_defines_and_paths_with_spaces() {
        let sfz = r#"
/* Salamander-style layout */
<control> default_path=48khz24bit\
#define $REL 1.5
<global> ampeg_release=$REL
<group> lovel=1 hivel=64 // soft layer
<region> sample=A0 soft.flac lokey=21 hikey=23 pitch_keycenter=a0
<region> sample=C1v1.flac key=c1 volume=-3 tune=10 transpose=-1
<group> lovel=65 hivel=127 loop_mode=loop_continuous loop_start=100 loop_end=900
<region> sample=A0v16.flac lokey=21 hikey=23 pitch_keycenter=21 ampeg_release=0.2
<group> trigger=release
<region> sample=rel1.flac lokey=21 hikey=108
"#;
        let r = parse_sfz(sfz, Path::new("/packs/salamander")).expect("parse");
        assert_eq!(r.len(), 3, "release-trigger region skipped");
        assert_eq!(
            r[0].sample,
            Path::new("/packs/salamander/48khz24bit/A0 soft.flac")
        );
        assert_eq!((r[0].lokey, r[0].hikey, r[0].keycenter), (21, 23, 21));
        assert_eq!((r[0].lovel, r[0].hivel), (1, 64));
        assert_eq!(r[0].release_s, Some(1.5));
        assert_eq!((r[1].lokey, r[1].hikey, r[1].keycenter), (24, 24, 24));
        assert_eq!(r[1].volume_db, -3.0);
        assert_eq!(r[1].tune_cents, -90.0);
        assert_eq!((r[2].lovel, r[2].hivel), (65, 127));
        assert_eq!(r[2].release_s, Some(0.2), "region overrides global");
        assert_eq!(r[2].loop_frames, Some((100, 900)));
    }

    #[test]
    fn empty_files_are_an_error() {
        assert!(parse_sfz("<group> lovel=1", Path::new(".")).is_err());
    }
}
