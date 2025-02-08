use num_complex::Complex32;
use palette::Hwb;

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

// adjust the amplitude to account for the sound perception
// using the Equal Loudness Contour (ISO 226)
// values from github.com/musios-app/equal-loudness
fn equalize(a: f32, f: u32) -> f32 {
    match f {
        x if x <= 23 => 1.0 / 2.50 * a,
        x if x <= 28 => 1.0 / 2.35 * a,
        x if x <= 36 => 1.0 / 2.20 * a,
        x if x <= 45 => 1.0 / 2.07 * a,
        x if x <= 57 => 1.0 / 1.94 * a,
        x if x <= 72 => 1.0 / 1.83 * a,
        x if x <= 91 => 1.0 / 1.71 * a,
        x if x <= 113 => 1.0 / 1.61 * a,
        x if x <= 142 => 1.0 / 1.51 * a,
        x if x <= 180 => 1.0 / 1.42 * a,
        x if x <= 225 => 1.0 / 1.34 * a,
        x if x <= 283 => 1.0 / 1.26 * a,
        x if x <= 352 => 1.0 / 1.19 * a,
        x if x <= 450 => 1.0 / 1.12 * a,
        x if x <= 565 => 1.0 / 1.08 * a,
        x if x <= 715 => 1.0 / 1.03 * a,
        x if x <= 1125 => a,
        x if x <= 1425 => 1.0 / 1.05 * a,
        x if x <= 1800 => 1.0 / 1.06 * a,
        x if x <= 2250 => 1.0 / 0.98 * a,
        x if x <= 2825 => 1.0 / 0.91 * a,
        x if x <= 3575 => 1.0 / 0.89 * a,
        x if x <= 4500 => 1.0 / 0.92 * a,
        x if x <= 5650 => a,
        x if x <= 7150 => 1.0 / 1.15 * a,
        x if x <= 9000 => 1.0 / 1.30 * a,
        x if x <= 11_250 => 1.0 / 1.36 * a,
        x if x <= 14_250 => 1.0 / 1.29 * a,
        x if x <= 18_000 => 1.0 / 1.30 * a,
        _ => 1.0 / 2.32 * a,
    }
}

pub fn update_colors(
    colors: &mut [Hwb],
    spectrum: Vec<f32>,
    min_freq: u16,
    max_freq: u16,
    hz_per_bin: u32,
    prev_max: &mut [f32],
    derivative: &mut [f32],
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
    let rate_func_dec = |x: f32| -> f32 { 0.8 * (1.0 - (1.0 - x).powi(4)) };

    for (i, ampl) in spectrum.into_iter().enumerate() {
        let cur_freq = (i as u32) * hz_per_bin + hz_per_bin / 2;
        if cur_freq > intervals[cur_interval] || i == n_bins - 1 {
            let ampl_delta = cur_max - prev_max[cur_interval];
            if ampl_delta > f32::EPSILON {
                derivative[cur_interval] = -rate_func_inc(ampl_delta);
            } else if ampl_delta < -f32::EPSILON {
                derivative[cur_interval] = rate_func_dec(-ampl_delta);
            }
            colors[cur_interval].blackness =
                (colors[cur_interval].blackness + derivative[cur_interval]).clamp(0.0, 1.0);

            prev_max[cur_interval] = cur_max;
            cur_max = 0.0;
            cur_interval += 1;
        }
        if cur_freq > *intervals.last().unwrap() {
            break;
        }
        cur_max = cur_max.max(equalize(ampl, cur_freq).min(1.0));
    }
}
