use crate::utils;
use num_complex::Complex32;
use palette::Hwb;

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

/// Updates the blackness component of HWB colors based on audio frequency spectrum for animated visualization.
///
/// Divides the frequency range [min_freq, max_freq] into `colors.len()` logarithmic intervals.
/// For each interval, tracks maximum amplitude (with equal loudness correction) and updates blackness
/// with velocity-based decay/increase for smooth transitions. Uses cubic easing functions for rates.
///
/// # Arguments
///
/// * `spectrum` - FFT-derived amplitudes for frequency bins.
/// * `hz_per_bin` - Frequency resolution (Hz per bin in spectrum).
/// * `min_freq`/`max_freq` - Frequency range to consider for color mapping.
/// * `colors` - Mutable slice of HWB colors; updates their blackness [0,1] (1=white, 0=saturated).
/// * `prev_max` - Previous max amplitudes per interval for delta computation (mutated).
/// * `speed` - Velocity accumulators for blackness changes per interval (mutated).
///
/// # Panics
///
/// May panic if spectrum length insufficient or invalid frequency params.
pub fn update_colors(
    spectrum: Vec<f32>,
    hz_per_bin: u32,
    min_freq: u16,
    max_freq: u16,
    colors: &mut [Hwb],
    prev_max: &mut [f32],
    speed: &mut [f32],
) {
    let n_panels = colors.len();
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
            if ampl_delta.is_sign_positive() {
                speed[cur_interval] = -rate_func_inc(ampl_delta);
            } else {
                speed[cur_interval] = rate_func_dec(-ampl_delta).max(1e-4);
            }
            colors[cur_interval].blackness =
                (colors[cur_interval].blackness + speed[cur_interval]).clamp(0.0, 1.0);

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
