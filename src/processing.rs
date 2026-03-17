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

/// Updates per-panel brightness using an energy wave / cascade animation.
///
/// Instead of each panel independently tracking a frequency band (like `update_brightness`),
/// this effect creates a traveling wave of light across the panels:
///
/// 1. Compute overall audio energy from the full spectrum (max equalized amplitude).
/// 2. Cascade: shift each panel's brightness to the next panel (with per-step decay).
/// 3. Feed new energy into the first panel with smooth attack/decay.
///
/// The result is a flowing ripple of light that propagates across panels,
/// with intensity driven by audio amplitude. Visually very different from the
/// per-band spectrum effect.
///
/// # Arguments
///
/// * `spectrum` - FFT-derived amplitudes for frequency bins.
/// * `hz_per_bin` - Frequency resolution (Hz per bin in spectrum).
/// * `min_freq`/`max_freq` - Frequency range to consider.
/// * `brightness` - Mutable slice of brightness multipliers [0,1] per panel (mutated).
/// * `prev_max` - Previous overall energy for delta computation (only index 0 used).
/// * `speed` - Velocity accumulator for the lead panel's brightness (only index 0 used).
pub fn update_brightness_wave(
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

    // 1. Compute overall audio energy: max equalized amplitude in the frequency range
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

    // 2. Cascade: shift brightness values from left to right with per-step decay
    // This creates the traveling wave — each panel inherits its left neighbor's value
    let cascade_decay = 0.92;
    for i in (1..n_panels).rev() {
        brightness[i] = brightness[i - 1] * cascade_decay;
    }

    // 3. Feed new energy into the lead panel (index 0) with smooth attack/decay
    let rate_func_inc = |x: f32| -> f32 { 1.0 - (1.0 - x).powi(3) };
    let rate_func_dec = |x: f32| -> f32 { 0.9 * (1.0 - (1.0 - x).powi(4)) };

    let energy_delta = overall_energy - prev_max[0];
    if energy_delta > 0.0 {
        speed[0] = rate_func_inc(energy_delta);
    } else if overall_energy > 0.01 {
        // Audio getting quieter but still present → normal decay
        speed[0] = -(rate_func_dec(-energy_delta).max(0.01));
    } else {
        // Audio is essentially silent → strong decay so panels actually go dark
        speed[0] = -(0.15_f32.max(brightness[0] * 0.3));
    }
    brightness[0] = (brightness[0] + speed[0]).clamp(0.0, 1.0);
    prev_max[0] = overall_energy;

    // Floor very small values to true zero so panels go fully dark
    for b in brightness.iter_mut() {
        if *b < 0.005 {
            *b = 0.0;
        }
    }
}

/// Updates all panels with a unified beat-reactive pulse animation.
///
/// All panels flash together on audio transients (beats, hits, kicks) and
/// fade quickly between them. The music's own rhythm drives the animation —
/// no fixed-rate oscillation.
///
/// Uses asymmetric attack/decay with signal boost:
/// - **Boost**: `sqrt(energy)` expands the dynamic range so moderate audio
///   levels (0.25 → 0.5, 0.5 → 0.71) still produce visible flashes.
/// - **Attack**: When boosted energy exceeds current brightness, snap to it
///   almost instantly (90% of the gap per frame). Every kick/snare/transient
///   produces an immediate flash.
/// - **Decay**: When energy drops, brightness decays exponentially (×0.72
///   per frame ≈ 250ms to half-brightness at 5.3 fps). The fast falloff
///   creates strong contrast between beats — panels go noticeably dark
///   before the next hit lands.
///
/// State layout:
/// - `prev_max[0]`: current display brightness level (smoothed)
///
/// # Arguments
///
/// * `spectrum` - FFT-derived amplitudes for frequency bins.
/// * `hz_per_bin` - Frequency resolution (Hz per bin in spectrum).
/// * `min_freq`/`max_freq` - Frequency range to consider.
/// * `brightness` - Mutable slice of brightness multipliers [0,1] per panel (all set to same value).
/// * `prev_max` - Index 0 stores the current pulse brightness across frames.
/// * `speed` - Unused by this effect (reserved for compatibility).
pub fn update_brightness_pulse(
    spectrum: Vec<f32>,
    hz_per_bin: u32,
    min_freq: u16,
    max_freq: u16,
    brightness: &mut [f32],
    prev_max: &mut [f32],
    _speed: &mut [f32],
) {
    if brightness.is_empty() {
        return;
    }
    let (min_freq, max_freq) = (min_freq as u32, max_freq as u32);

    // 1. Compute overall audio energy: max equalized amplitude in the frequency range
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

    // 2. Boost signal: sqrt expands the dynamic range so moderate levels produce visible pulses
    //    Without this, typical music energy (0.2-0.5) barely lights the panels
    let boosted = overall_energy.sqrt();

    // 3. Asymmetric attack/decay for punchy beat-reactive pulse
    if boosted > prev_max[0] {
        // Near-instant attack: snap 90% of the gap per frame
        // Every transient produces an immediate flash
        prev_max[0] += 0.9 * (boosted - prev_max[0]);
    } else {
        // Fast exponential decay between beats
        // 0.72 per frame at ~5.3 fps ≈ 250ms to half-brightness
        // Creates strong contrast so the next beat has real impact
        prev_max[0] *= 0.72;
    }

    // Floor very small values to true zero so panels go fully dark
    if prev_max[0] < 0.005 {
        prev_max[0] = 0.0;
    }

    // 4. All panels pulse together at the same brightness
    brightness.fill(prev_max[0]);
}
