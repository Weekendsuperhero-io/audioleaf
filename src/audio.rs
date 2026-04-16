use crate::constants;
use anyhow::{Result, bail};
use cpal::{Device, SampleFormat, StreamConfig, traits::*};
use hashbrown::HashMap;

pub struct AudioStream {
    pub device: Device,
    pub sample_format: SampleFormat,
    pub stream_config: StreamConfig,
}

#[derive(Clone)]
struct InputDeviceEntry {
    device: Device,
    backend_name: String,
    friendly_name: String,
    legacy_display_name: String,
}

impl AudioStream {
    /// Creates a new `AudioStream` instance for capturing audio from an input device.
    ///
    /// Prioritizes loopback/monitor devices for system-wide audio capture (e.g., BlackHole on macOS).
    /// Searches common names like "BlackHole", "Loopback Audio", etc.
    /// Falls back to the system's default input device (often microphone) with a warning.
    /// Supports specifying a custom device name via `device_name`.
    ///
    /// # Arguments
    ///
    /// * `device_name` - Optional name of the audio device to use. Defaults to automatic loopback detection.
    ///
    /// # Returns
    ///
    /// `Result<Self>` with the configured `AudioStream` containing device, sample format, and config.
    ///
    /// # Errors
    ///
    /// Propagates `cpal` errors for device discovery or config retrieval. Bail with available devices list if none match.
    pub fn new(device_name: Option<&str>) -> Result<Self> {
        let requested_name = match device_name {
            Some(name) => name,
            None => constants::DEFAULT_AUDIO_BACKEND,
        };
        let host = cpal::default_host();
        let input_devices = enumerate_input_devices(&host)?;

        // Try to find the device in input devices (for loopback/monitor devices)
        let device = match requested_name {
            constants::DEFAULT_AUDIO_BACKEND => {
                // Check for common loopback device names first
                let loopback_names = [
                    "BlackHole",
                    "BlackHole 2ch",
                    "BlackHole 16ch",
                    "Loopback Audio",
                    "CABLE Output",
                    "VB-Audio",
                    "Monitor",
                    "monitor",
                ];

                let mut loopback_device = None;
                for entry in &input_devices {
                    if loopback_names.iter().any(|lb| {
                        entry.friendly_name.contains(lb) || entry.backend_name.contains(lb)
                    }) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "INFO: Found loopback device: {} ({})",
                            entry.friendly_name, entry.backend_name
                        );
                        loopback_device = Some(entry.device.clone());
                        break;
                    }
                }

                // Fall back to default input if no loopback found
                loopback_device.or_else(|| {
                    eprintln!("WARNING: No loopback device found. Using default input device (microphone).");
                    eprintln!("To capture system audio on macOS, install BlackHole: https://github.com/ExistentialAudio/BlackHole");
                    host.default_input_device()
                })
            }
            _ => select_input_device(&input_devices, requested_name).map(|entry| entry.device),
        };

        let Some(device) = device else {
            bail!(format!(
                "Audio backend `{}` not found, available options: {}",
                requested_name,
                list_input_backend_names()?.join(", ")
            ));
        };
        let (sample_format, stream_config) = select_input_stream_profile(&device, requested_name)?;

        Ok(AudioStream {
            device,
            sample_format,
            stream_config,
        })
    }
}

/// Returns input backend names with stable disambiguation suffixes for duplicates.
///
/// Example:
/// - "Loopback, Loopback PCM [#1]"
/// - "Loopback, Loopback PCM [#2]"
pub fn list_input_backend_names() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let devices = enumerate_input_devices(&host)?;
    Ok(devices
        .into_iter()
        .map(|entry| entry.backend_name)
        .collect())
}

fn enumerate_input_devices(host: &cpal::Host) -> Result<Vec<InputDeviceEntry>> {
    let raw_devices: Vec<(Device, String, String)> = host
        .input_devices()?
        .filter_map(|device| {
            let backend_name = device
                .id()
                .ok()
                .map(|id| id.1.trim().to_string())
                .filter(|name| !name.is_empty());
            let friendly_name = device
                .description()
                .ok()
                .map(|description| description.name().trim().to_string())
                .filter(|name| !name.is_empty());

            let backend_name = match (backend_name, friendly_name.as_deref()) {
                (Some(name), _) => name,
                (None, Some(name)) => name.to_string(),
                (None, None) => return None,
            };
            let friendly_name = friendly_name.unwrap_or_else(|| backend_name.clone());
            Some((device, backend_name, friendly_name))
        })
        .collect();

    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, _, friendly_name) in &raw_devices {
        *counts.entry(friendly_name.clone()).or_default() += 1;
    }

    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut result = Vec::with_capacity(raw_devices.len());
    for (device, backend_name, friendly_name) in raw_devices {
        let total = counts.get(&friendly_name).copied().unwrap_or(1);
        let legacy_display_name = if total > 1 {
            let next = seen.entry(friendly_name.clone()).or_default();
            *next += 1;
            format!("{} [#{}]", friendly_name, *next)
        } else {
            friendly_name.clone()
        };
        result.push(InputDeviceEntry {
            device,
            backend_name,
            friendly_name,
            legacy_display_name,
        });
    }

    Ok(result)
}

fn select_input_device(
    devices: &[InputDeviceEntry],
    selected_name: &str,
) -> Option<InputDeviceEntry> {
    let selected_name = selected_name.trim();
    if selected_name.is_empty() {
        return None;
    }

    if let Some(entry) = devices.iter().find(|entry| {
        entry.backend_name == selected_name
            || entry.friendly_name == selected_name
            || entry.legacy_display_name == selected_name
    }) {
        return Some(entry.clone());
    }

    let selected_without_host_prefix = strip_host_prefix(selected_name);
    if selected_without_host_prefix != selected_name
        && let Some(entry) = devices.iter().find(|entry| {
            entry.backend_name == selected_without_host_prefix
                || entry.friendly_name == selected_without_host_prefix
                || entry.legacy_display_name == selected_without_host_prefix
        })
    {
        return Some(entry.clone());
    }

    if let Some((base_name, duplicate_index)) = parse_indexed_name(selected_name) {
        let mut matched_index = 0usize;
        for entry in devices {
            if entry.friendly_name == base_name {
                matched_index += 1;
                if matched_index == duplicate_index {
                    return Some(entry.clone());
                }
            }
        }
    }

    let candidate_names = expand_selector_candidates(selected_name);
    devices
        .iter()
        .find(|entry| candidate_names.contains(&entry.backend_name))
        .cloned()
}

fn select_input_stream_profile(
    device: &Device,
    requested_name: &str,
) -> Result<(SampleFormat, StreamConfig)> {
    if should_prefer_loopback_profile(device, requested_name)
        && let Some(profile) = pick_loopback_44100_profile(device)?
    {
        return Ok((profile.sample_format(), profile.config()));
    }

    let audio_config = device.default_input_config()?;
    Ok((audio_config.sample_format(), audio_config.into()))
}

fn should_prefer_loopback_profile(device: &Device, requested_name: &str) -> bool {
    let normalized = requested_name.to_ascii_lowercase();
    if normalized == constants::DEFAULT_AUDIO_BACKEND {
        return false;
    }

    if normalized.contains("loopback") || normalized.contains("aloop") {
        return true;
    }

    if let Ok(description) = device.description()
        && description.name().to_ascii_lowercase().contains("loopback")
    {
        return true;
    }

    if let Ok(device_id) = device.id()
        && device_id.1.to_ascii_lowercase().contains("loopback")
    {
        return true;
    }

    false
}

fn pick_loopback_44100_profile(device: &Device) -> Result<Option<cpal::SupportedStreamConfig>> {
    let mut best: Option<(u8, cpal::SupportedStreamConfig)> = None;
    for range in device.supported_input_configs()? {
        if range.channels() != 2 {
            continue;
        }
        if !(range.min_sample_rate() <= 44_100 && range.max_sample_rate() >= 44_100) {
            continue;
        }

        let score = match range.sample_format() {
            // Prefer matching Shairport's common ALSA loopback format exactly.
            SampleFormat::I16 => 0,
            // F32 is typically robust if I16 is unavailable.
            SampleFormat::F32 => 1,
            _ => 2,
        };
        let candidate = range.with_sample_rate(44_100);
        let should_replace = match &best {
            None => true,
            Some((best_score, _)) => score < *best_score,
        };
        if should_replace {
            best = Some((score, candidate));
        }
    }

    Ok(best.map(|(_, config)| config))
}

fn parse_indexed_name(name: &str) -> Option<(String, usize)> {
    if !name.ends_with(']') {
        return None;
    }
    let marker_start = name.rfind(" [#")?;
    let index_part = &name[(marker_start + 3)..(name.len() - 1)];
    let parsed_index = index_part.parse::<usize>().ok()?;
    if parsed_index == 0 {
        return None;
    }
    let base_name = name[..marker_start].to_string();
    if base_name.is_empty() {
        return None;
    }
    Some((base_name, parsed_index))
}

fn strip_host_prefix(name: &str) -> &str {
    let Some((prefix, remainder)) = name.split_once(':') else {
        return name;
    };
    if prefix.eq_ignore_ascii_case("alsa")
        || prefix.eq_ignore_ascii_case("coreaudio")
        || prefix.eq_ignore_ascii_case("wasapi")
        || prefix.eq_ignore_ascii_case("asio")
        || prefix.eq_ignore_ascii_case("jack")
        || prefix.eq_ignore_ascii_case("pipewire")
        || prefix.eq_ignore_ascii_case("aaudio")
        || prefix.eq_ignore_ascii_case("webaudio")
        || prefix.eq_ignore_ascii_case("emscripten")
        || prefix.eq_ignore_ascii_case("null")
    {
        return remainder.trim();
    }
    name
}

fn expand_selector_candidates(selected_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_candidate(&mut candidates, selected_name);

    let Some((prefix_raw, remainder_raw)) = selected_name.split_once(':') else {
        return candidates;
    };
    let prefix = prefix_raw.trim().to_ascii_lowercase();
    if prefix != "hw" && prefix != "plughw" {
        return candidates;
    }
    let remainder = remainder_raw.trim();
    if remainder.is_empty() {
        return candidates;
    }

    let Some((card, dev, subdev)) = parse_alsa_selector_parts(remainder) else {
        return candidates;
    };
    let normalized_prefix = prefix.as_str();
    push_candidate(
        &mut candidates,
        &format!("{}:{},{}", normalized_prefix, card, dev),
    );
    push_candidate(
        &mut candidates,
        &format!("{}:CARD={},DEV={}", normalized_prefix, card, dev),
    );

    if let Some(subdev) = subdev.as_deref() {
        push_candidate(
            &mut candidates,
            &format!("{}:{},{},{}", normalized_prefix, card, dev, subdev),
        );
        if subdev == "0" {
            push_candidate(
                &mut candidates,
                &format!("{}:{},{}", normalized_prefix, card, dev),
            );
        } else {
            push_candidate(
                &mut candidates,
                &format!(
                    "{}:CARD={},DEV={},SUBDEV={}",
                    normalized_prefix, card, dev, subdev
                ),
            );
        }
    } else {
        push_candidate(
            &mut candidates,
            &format!("{}:{},{},0", normalized_prefix, card, dev),
        );
    }

    if !is_ascii_digits(&card)
        && let Some(card_index) = lookup_alsa_card_index(&card)
    {
        push_candidate(
            &mut candidates,
            &format!("{}:{},{}", normalized_prefix, card_index, dev),
        );
        push_candidate(
            &mut candidates,
            &format!("{}:CARD={},DEV={}", normalized_prefix, card_index, dev),
        );
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<String>, candidate: &str) {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return;
    }
    if candidates.iter().any(|existing| existing == trimmed) {
        return;
    }
    candidates.push(trimmed.to_string());
}

fn parse_alsa_selector_parts(remainder: &str) -> Option<(String, String, Option<String>)> {
    if remainder.contains("CARD=") || remainder.contains("DEV=") {
        parse_named_alsa_selector_parts(remainder)
    } else {
        parse_short_alsa_selector_parts(remainder)
    }
}

fn parse_short_alsa_selector_parts(remainder: &str) -> Option<(String, String, Option<String>)> {
    let mut parts = remainder.split(',').map(str::trim);
    let card = parts.next()?.to_string();
    let dev = parts.next()?.to_string();
    if card.is_empty() || dev.is_empty() {
        return None;
    }
    let subdev = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some((card, dev, subdev))
}

fn parse_named_alsa_selector_parts(remainder: &str) -> Option<(String, String, Option<String>)> {
    let mut card: Option<String> = None;
    let mut dev: Option<String> = None;
    let mut subdev: Option<String> = None;

    for token in remainder.split(',') {
        let token = token.trim();
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_uppercase();
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "CARD" => card = Some(value),
            "DEV" => dev = Some(value),
            "SUBDEV" => subdev = Some(value),
            _ => {}
        }
    }

    Some((card?, dev?, subdev))
}

fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

#[cfg(target_os = "linux")]
fn lookup_alsa_card_index(card_name: &str) -> Option<usize> {
    let cards = std::fs::read_to_string("/proc/asound/cards").ok()?;
    for line in cards.lines() {
        let line = line.trim_start();
        let Some(bracket_start) = line.find('[') else {
            continue;
        };
        let Some(bracket_end_offset) = line[bracket_start + 1..].find(']') else {
            continue;
        };
        let bracket_end = bracket_start + 1 + bracket_end_offset;
        let short_name = line[bracket_start + 1..bracket_end].trim();
        if !short_name.eq_ignore_ascii_case(card_name) {
            continue;
        }
        let Some(index_str) = line.split_whitespace().next() else {
            continue;
        };
        if let Ok(index) = index_str.parse::<usize>() {
            return Some(index);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn lookup_alsa_card_index(_card_name: &str) -> Option<usize> {
    None
}
