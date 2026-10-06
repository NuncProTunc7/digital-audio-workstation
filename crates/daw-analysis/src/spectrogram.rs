use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

const FFT_SIZE: usize = 2048;
const MIN_HZ: f64 = 30.0;
const MAX_HZ: f64 = 20_000.0;
const RANGE_DB: f32 = 80.0;

/// Draws a spectrogram (time left→right, frequency bottom→top on a log
/// scale from 30 Hz to 20 kHz with dashed guides at 100 Hz, 1 kHz, and
/// 10 kHz, brightness = loudness) as a PNG. Lets Claude *see* the mix.
pub fn spectrogram_png(stereo: &[f32], sample_rate_hz: u32, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let mono: Vec<f32> = stereo
        .chunks_exact(2)
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
        .collect();
    let bin_hz = f64::from(sample_rate_hz) / FFT_SIZE as f64;
    // Which FFT bin each pixel row shows (log frequency).
    let row_bin: Vec<usize> = (0..h)
        .map(|y| {
            let t = 1.0 - y as f64 / (h - 1).max(1) as f64;
            let hz = MIN_HZ * (MAX_HZ / MIN_HZ).powf(t);
            ((hz / bin_hz).round() as usize).clamp(1, FFT_SIZE / 2 - 1)
        })
        .collect();

    let mut columns = vec![vec![f32::NEG_INFINITY; h]; w];
    let mut buf = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];
    let mut loudest = f32::NEG_INFINITY;
    for (x, column) in columns.iter_mut().enumerate() {
        let center = x * mono.len() / w;
        let start = center.saturating_sub(FFT_SIZE / 2);
        for (i, b) in buf.iter_mut().enumerate() {
            let s = mono.get(start + i).copied().unwrap_or(0.0);
            *b = Complex::new(s * window[i], 0.0);
        }
        fft.process(&mut buf);
        for (y, &bin) in row_bin.iter().enumerate() {
            let db = 10.0 * buf[bin].norm_sqr().max(1e-20).log10();
            column[y] = db;
            loudest = loudest.max(db);
        }
    }

    // Faint guide lines at 100 Hz, 1 kHz, and 10 kHz.
    let guide_rows: Vec<usize> = [100.0f64, 1_000.0, 10_000.0]
        .iter()
        .map(|hz| {
            let t = (hz / MIN_HZ).ln() / (MAX_HZ / MIN_HZ).ln();
            ((1.0 - t) * (h - 1) as f64).round() as usize
        })
        .collect();
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let guide = guide_rows.contains(&y);
        for (x, column) in columns.iter().enumerate() {
            let t = ((column[y] - loudest + RANGE_DB) / RANGE_DB).clamp(0.0, 1.0);
            let mut px = color(t);
            if guide && x % 4 < 2 {
                px = px.map(|c| c.saturating_add(70));
            }
            rgb.extend_from_slice(&px);
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(&rgb);
        }
    }
    out
}

/// Dark purple → magenta → amber → near white.
fn color(t: f32) -> [u8; 3] {
    const STOPS: [(f32, [f32; 3]); 4] = [
        (0.0, [12.0, 10.0, 28.0]),
        (0.45, [120.0, 40.0, 140.0]),
        (0.8, [245.0, 160.0, 50.0]),
        (1.0, [255.0, 245.0, 220.0]),
    ];
    for pair in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            return [0, 1, 2].map(|i| (c0[i] + (c1[i] - c0[i]) * f) as u8);
        }
    }
    [255, 245, 220]
}
