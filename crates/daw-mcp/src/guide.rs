//! The user guide (`docs/guide`), built into the bridge so Claude can read it
//! wherever it runs: Claude Desktop has no copy of the repository.

/// Each page: its name (the file name without `.md`) and text.
pub const PAGES: &[(&str, &str)] = &[
    ("README", include_str!("../../../docs/guide/README.md")),
    ("lessons", include_str!("../../../docs/guide/lessons.md")),
    ("basics", include_str!("../../../docs/guide/basics.md")),
    (
        "writing-music",
        include_str!("../../../docs/guide/writing-music.md"),
    ),
    ("plugins", include_str!("../../../docs/guide/plugins.md")),
    ("audio", include_str!("../../../docs/guide/audio.md")),
    ("mixing", include_str!("../../../docs/guide/mixing.md")),
    (
        "sheet-music",
        include_str!("../../../docs/guide/sheet-music.md"),
    ),
    ("godot", include_str!("../../../docs/guide/godot.md")),
    ("claude", include_str!("../../../docs/guide/claude.md")),
    (
        "composing",
        include_str!("../../../docs/guide/composing.md"),
    ),
    (
        "troubleshooting",
        include_str!("../../../docs/guide/troubleshooting.md"),
    ),
];

/// The page called `name` (with or without `.md`); none means the contents.
pub fn read(name: Option<&str>) -> Result<&'static str, String> {
    let name = name
        .map(|n| n.trim().trim_end_matches(".md"))
        .filter(|n| !n.is_empty())
        .unwrap_or("README");
    PAGES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, text)| *text)
        .ok_or_else(|| {
            let names: Vec<&str> = PAGES.iter().map(|(n, _)| *n).collect();
            format!("no guide page {name:?}; pages: {}", names.join(", "))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_in_the_folder_is_built_in_and_links_resolve() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/guide");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .expect("guide folder")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|f| f.ends_with(".md"))
            .collect();
        files.sort();
        let mut built_in: Vec<String> = PAGES.iter().map(|(n, _)| format!("{n}.md")).collect();
        built_in.sort();
        assert_eq!(files, built_in, "add new guide pages to PAGES");
        for (page, text) in PAGES {
            for link in text.split("](").skip(1) {
                let target = link.split(')').next().unwrap_or("");
                if target.ends_with(".md") {
                    assert!(
                        read(Some(target)).is_ok(),
                        "{page} links to missing {target}"
                    );
                }
            }
        }
    }

    #[test]
    fn defaults_to_the_contents_and_names_pages_on_a_miss() {
        assert!(
            read(None)
                .expect("contents")
                .starts_with("# Nunc Pro Tune guide")
        );
        assert!(read(Some("Lessons.md")).is_ok());
        let err = read(Some("nope")).expect_err("missing");
        assert!(err.contains("composing"));
    }
}
