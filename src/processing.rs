use crate::utils;
use num_complex::Complex32;

/// Recursive in-place radix-2 Cooley-Tukey FFT implementation.
///
/// Computes the discrete Fourier transform for a signal of length `n`.
/// Modifies input `x` and stores result in `y`.
/// `step` determines the stride between samples in sub-transforms for decimation-in-time approach.
///
/// Base case: n=1 copies x[0] to y[0].
/// Recursive: splits into even/odd indices, computes twiddle factors for butterfly combination.
fn fft(x: &mut [Complex32], y: &mut [Complex32], n: usize, step: usize) {
    if n == 1 {
        y[0] = x[0];
        return;
    }
    fft(x, y, n / 2, step * 2);
    fft(&mut x[step..], &mut y[(n / 2)..], n / 2, step * 2);
    for k in 0..(n / 2) {
        let t = (-2.0 * Complex32::I * std::f32::consts::PI * (k as f32) / (n as f32)).exp()
            * y[k + n / 2];
        let temp = y[k];
        y[k] = temp + t;
        y[k + n / 2] = temp - t;
    }
}

/// Processes raw time-domain audio samples to produce frequency-domain spectrum amplitudes for visualization.
///
/// Performs FFT on padded input (to next power of two), extracts positive frequencies,
/// normalizes by sqrt(n), applies gain, and clamps amplitudes to [0,1] using x / sqrt(1 + x^2) sigmoid-like function.
///
/// # Arguments
///
/// * `samples` - Vec of f32 mono audio samples.
/// * `gain` - Amplification factor applied before clamping.
///
/// # Returns
///
/// Vec<f32> of amplitude values for each frequency bin (up to Nyquist).
pub fn process(samples: Vec<f32>, gain: f32) -> Vec<f32> {
    let mut n = samples.len();
    let mut complex_samples = samples
        .into_iter()
        .map(|x| Complex32::new(x, 0.0))
        .collect::<Vec<_>>();
    complex_samples.append(&mut vec![
        Complex32::new(0.0, 0.0);
        n.next_power_of_two() - n
    ]);
    n = complex_samples.len();
    let mut transformed_samples = complex_samples.clone();
    fft(&mut complex_samples, &mut transformed_samples, n, 1);

    // normalize and apply a sigmoid-type function (x / sqrt(1 + x^2)) to clamp amplitudes between 0 and 1
    let root_n = (n as f32).sqrt();
    transformed_samples
        .into_iter()
        .take(n / 2)
        .map(|z| {
            let x = gain * z.norm() / root_n;
            x / (1.0 + x * x).sqrt()
        })
        .collect::<Vec<_>>()
}

/// Updates per-panel brightness values based on audio frequency spectrum for animated visualization.
///
/// Divides the frequency range [min_freq, max_freq] into `brightness.len()` logarithmic intervals.
/// For each interval, tracks maximum amplitude (with equal loudness correction) and updates brightness
/// with velocity-based decay/increase for smooth transitions. Uses cubic easing functions for rates.
///
/// Brightness is a multiplier in [0,1] applied to the base Oklch color's lightness.
/// At 0 the panel is black; at 1 it shows the exact target color the user specified.
///
/// # Arguments
///
/// * `spectrum` - FFT-derived amplitudes for frequency bins.
/// * `hz_per_bin` - Frequency resolution (Hz per bin in spectrum).
/// * `min_freq`/`max_freq` - Frequency range to consider for color mapping.
/// * `brightness` - Mutable slice of brightness multipliers [0,1] per panel (mutated).
/// * `prev_max` - Previous max amplitudes per interval for delta computation (mutated).
/// * `speed` - Velocity accumulators for brightness changes per interval (mutated).
///
/// # Panics
///
/// May panic if spectrum length insufficient or invalid frequency params.
pub fn update_brightness(
    spectrum: Vec<f32>,
    hz_per_bin: u32,
    min_freq: u16,
    max_freq: u16,
    brightness: &mut [f32],
    prev_max: &mut [f32],
    speed: &mut [f32],
) {
    let n_panels = brightness.len();
    let (min_freq, max_freq) = (min_freq as f32, max_freq as f32);
    let multiplier = (max_freq / min_freq).powf(1.0 / n_panels as f32);
    let (mut intervals, mut cutoff) = (Vec::new(), min_freq);
    for _ in 0..n_panels {
        cutoff *= multiplier;
        intervals.push(cutoff.round().min(max_freq) as u32);
    }
    let (n_bins, mut cur_interval, mut cur_max) = (spectrum.len(), 0, 0.0_f32);
    let rate_func_inc = |x: f32| -> f32 { 1.0 - (1.0 - x).powi(3) };
    let rate_func_dec = |x: f32| -> f32 { 0.9 * (1.0 - (1.0 - x).powi(4)) };

    for (i, ampl) in spectrum.into_iter().enumerate() {
        let cur_freq = (i as u32) * hz_per_bin + hz_per_bin / 2;
        if cur_freq > intervals[cur_interval] || i == n_bins - 1 {
            let ampl_delta = cur_max - prev_max[cur_interval];
            if ampl_delta > 0.0 {
                // Audio getting louder → increase brightness
                speed[cur_interval] = rate_func_inc(ampl_delta);
            } else if cur_max > 0.01 {
                // Audio getting quieter but still present → normal decay
                speed[cur_interval] = -(rate_func_dec(-ampl_delta).max(0.01));
            } else {
                // Audio is essentially silent → strong decay so panels actually go dark
                // Without this, the tiny 1e-4 floor makes panels take minutes to fade out
                speed[cur_interval] = -(0.15_f32.max(brightness[cur_interval] * 0.3));
            }
            brightness[cur_interval] =
                (brightness[cur_interval] + speed[cur_interval]).clamp(0.0, 1.0);

            // Floor very small values to true zero so panels go fully dark
            if brightness[cur_interval] < 0.005 {
                brightness[cur_interval] = 0.0;
            }

            prev_max[cur_interval] = cur_max;
            cur_max = 0.0;
            cur_interval += 1;
        }
        if cur_freq > *intervals.last().unwrap() {
            break;
        }
        cur_max = cur_max.max(utils::equalize(ampl, cur_freq).min(1.0));
    }
}

/// Updates per-panel brightness using per-band tracking with spatial bleed.
///
/// Like `update_brightness` (Spectrum), each panel tracks its own logarithmic
/// frequency band.  The difference is a neighbor-bleed pass after the per-band
/// update: each panel's brightness is mixed with its left and right neighbors.
/// This creates a flowing, wave-like appearance while preserving individual
/// panel reactivity so all palette colors reach full brightness.
///
/// # Arguments
///
/// * `spectrum` - FFT-derived amplitudes for frequency bins.
/// * `hz_per_bin` - Frequency resolution (Hz per bin in spectrum).
/// * `min_freq`/`max_freq` - Frequency range to consider.
/// * `brightness` - Mutable slice of brightness multipliers [0,1] per panel (mutated).
/// * `prev_max` - Previous max amplitudes per interval (mutated).
/// * `speed` - Velocity accumulators per interval (mutated).
pub fn update_brightness_wave(
    spectrum: Vec<f32>,
    hz_per_bin: u32,
    min_freq: u16,
    max_freq: u16,
    brightness: &mut [f32],
    prev_max: &mut [f32],
    speed: &mut [f32],
) {
    // First: identical per-band tracking as Spectrum
    update_brightness(
        spectrum, hz_per_bin, min_freq, max_freq, brightness, prev_max, speed,
    );

    // Second: spatial bleed — mix each panel with its neighbors for a flowing wave look.
    // Two passes (left→right, right→left) to propagate energy in both directions.
    let bleed = 0.25_f32; // how much a neighbor contributes
    let n = brightness.len();
    if n < 2 {
        return;
    }
    // Snapshot before bleed so we read original values
    let snap: Vec<f32> = brightness.to_vec();
    for i in 0..n {
        let left = if i > 0 { snap[i - 1] } else { 0.0 };
        let right = if i + 1 < n { snap[i + 1] } else { 0.0 };
        let neighbor_max = left.max(right);
        // Only bleed IN (raise brightness), never drag it down
        if neighbor_max > brightness[i] {
            brightness[i] += bleed * (neighbor_max - brightness[i]);
        }
    }
}

/// Onset-triggered ripples that propagate outward from panel 0.
///
/// Each audio transient (kick, snare, beat) spawns a bright wavefront at the
/// first panel.  The wavefront travels toward the last panel, stretching and
/// fading as it goes — like a ripple on water or a starship jumping to warp.
/// Multiple ripples overlap additively so rapid beats produce interference
/// patterns.
///
/// State layout:
/// - `speed[0..n]`    — the ripple wave field (amplitude at each panel position).
/// - `prev_max[0]`    — smoothed energy envelope for onset detection.
///
/// # Arguments
///
/// * `spectrum` - FFT-derived amplitudes for frequency bins.
/// * `hz_per_bin` - Frequency resolution (Hz per bin in spectrum).
/// * `min_freq`/`max_freq` - Frequency range to consider.
/// * `brightness` - Mutable slice of brightness multipliers [0,1] per panel (mutated).
/// * `prev_max` - Index 0: smoothed energy tracker (mutated).
/// * `speed` - Ripple wave field (mutated).
pub fn update_brightness_ripple(
    spectrum: Vec<f32>,
    hz_per_bin: u32,
    min_freq: u16,
    max_freq: u16,
    brightness: &mut [f32],
    prev_max: &mut [f32],
    speed: &mut [f32],
) {
    let n_panels = brightness.len();
    if n_panels == 0 {
        return;
    }
    let (min_freq, max_freq) = (min_freq as u32, max_freq as u32);

    // 1. Compute overall audio energy
    let mut overall_energy = 0.0_f32;
    for (i, &ampl) in spectrum.iter().enumerate() {
        let cur_freq = (i as u32) * hz_per_bin + hz_per_bin / 2;
        if cur_freq < min_freq {
            continue;
        }
        if cur_freq > max_freq {
            break;
        }
        overall_energy = overall_energy.max(utils::equalize(ampl, cur_freq).min(1.0));
    }
    let boosted = overall_energy.sqrt();

    // 2. Onset detection — a new ripple spawns when energy jumps sharply
    let onset_threshold = 0.08;
    let is_onset = boosted > prev_max[0] + onset_threshold && boosted > 0.10;
    // Envelope: fast attack, fast release so it drops between beats
    // At 0.55 per frame (~5.3 fps), halves in ~250ms — a 120bpm kick
    // (500ms apart) easily clears the threshold again.
    if boosted > prev_max[0] {
        prev_max[0] = prev_max[0] * 0.4 + boosted * 0.6;
    } else {
        prev_max[0] *= 0.55;
    }

    // 3. Propagate the ripple field outward — wavefront travels from panel 0 → N
    //    Per-step decay controls how far ripples reach and how sharp the edges are.
    let step_decay = 0.88;
    for i in (1..n_panels).rev() {
        speed[i] = speed[i - 1] * step_decay;
    }

    // 4. Inject at source
    if is_onset {
        // New ripple: bright flash. Additive so overlapping ripples reinforce.
        speed[0] = (speed[0] + boosted).min(1.0);
    } else {
        // Between onsets: fast fade at source so distinct rings separate
        speed[0] *= 0.35;
    }

    // 5. Output — leading-edge emphasis for that warp-stretch look
    for i in 0..n_panels {
        let ahead = if i + 1 < n_panels { speed[i + 1] } else { 0.0 };
        let edge_boost = if speed[i] > ahead * 1.2 { 1.2 } else { 1.0 };
        brightness[i] = (speed[i] * edge_boost).clamp(0.0, 1.0);
        if brightness[i] < 0.005 {
            brightness[i] = 0.0;
        }
    }
}
