//! Analyze a project file: `cargo run -p daw-analysis --example analyze_file -- song.nptune out.png`
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: analyze_file <project.nptune> [spectrogram.png]");
        std::process::exit(2);
    };
    let project = match daw_model::load_project(std::path::Path::new(path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let a = daw_analysis::analyze(
        &project,
        &daw_analysis::AnalyzeOptions {
            spectrogram: args.get(2).is_some(),
            ..Default::default()
        },
    );
    println!("{}", serde_json::to_string_pretty(&a).unwrap_or_default());
    if let (Some(out), Some(png)) = (args.get(2), a.spectrogram_png) {
        let _ = std::fs::write(out, png);
    }
}
