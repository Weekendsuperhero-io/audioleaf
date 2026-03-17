//! Album art color extraction for the visualizer.
//!
//! macOS: Uses ScriptingBridge.framework (via objc2) to query running media
//! players directly through the ObjC bridge — no subprocess spawning.
//!   - Spotify: title, artwork URL (downloaded via reqwest).
//!   - Apple Music: title, raw artwork bytes from MusicArtwork.rawData.
//!
//! Linux: Uses `playerctl` subprocess.

/// Debug-only logging (stripped from release builds).
macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        eprintln!($($arg)*);
    };
}

/// Returns the title of the currently playing track.
pub fn get_track_title() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::get_track_title()
    }
    #[cfg(target_os = "linux")]
    {
        linux::get_track_title()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Fetches artwork bytes once and returns both the raw image and the extracted palette.
/// Avoids double-fetch race conditions where the track could change between calls.
pub fn fetch_artwork_and_palette() -> Option<(Vec<u8>, Vec<[u8; 3]>)> {
    #[cfg(target_os = "macos")]
    {
        let bytes = macos::fetch_artwork_bytes()?;
        let colors = macos::extract_colors(&bytes)?;
        Some((bytes, colors))
    }
    #[cfg(target_os = "linux")]
    {
        let bytes = linux::fetch_artwork_bytes()?;
        let colors = linux::extract_colors_from_bytes(&bytes)?;
        Some((bytes, colors))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

// ── macOS — ScriptingBridge ──────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    #[link(name = "ScriptingBridge", kind = "framework")]
    unsafe extern "C" {}

    pub fn get_track_title() -> Option<String> {
        sb_spotify_title().or_else(sb_apple_music_title)
    }

    pub fn fetch_artwork_bytes() -> Option<Vec<u8>> {
        sb_spotify_artwork().or_else(sb_apple_music_artwork)
    }

    // ── ScriptingBridge helpers ───────────────────────────────────────────────

    /// Open a scripting bridge to a running application.
    /// Returns the app AND whether it is actively playing.
    /// Separated from player-state check so callers can decide if they need playing state.
    fn sb_app(bundle_id: &str) -> Option<Retained<AnyObject>> {
        unsafe {
            let cls = AnyClass::get(c"SBApplication")?;
            let bid = NSString::from_str(bundle_id);
            let app: Option<Retained<AnyObject>> =
                msg_send![cls, applicationWithBundleIdentifier: &*bid];
            let app = app?;
            let running: bool = msg_send![&*app, isRunning];
            if !running {
                debug_log!("DEBUG now_playing: {} not running", bundle_id);
                return None;
            }
            Some(app)
        }
    }

    /// Check if an app's player state is "playing" (four-char code 'kPSP').
    fn is_playing(app: &AnyObject) -> bool {
        unsafe {
            let state: u32 = msg_send![app, playerState];
            debug_log!("DEBUG now_playing: playerState = 0x{:08X}", state);
            // 'kPSP' = playing
            state == 0x6b505350
        }
    }

    // ── Spotify via ScriptingBridge ───────────────────────────────────────────

    fn sb_spotify_title() -> Option<String> {
        let app = sb_app("com.spotify.client")?;
        if !is_playing(&app) {
            return None;
        }
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;
            let name: Option<Retained<NSString>> = msg_send![&*track, name];
            let result = name.map(|s| s.to_string());
            debug_log!("DEBUG now_playing: Spotify title = {:?}", result);
            result
        }
    }

    fn sb_spotify_artwork() -> Option<Vec<u8>> {
        let app = sb_app("com.spotify.client")?;
        if !is_playing(&app) {
            return None;
        }
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;
            let url: Option<Retained<NSString>> = msg_send![&*track, artworkUrl];
            let url = url?;
            let url_str = url.to_string();
            debug_log!("DEBUG now_playing: Spotify artwork URL = {}", url_str);
            reqwest::blocking::get(&url_str)
                .ok()?
                .bytes()
                .ok()
                .map(|b| b.to_vec())
        }
    }

    // ── Apple Music via ScriptingBridge ───────────────────────────────────────

    fn sb_apple_music_title() -> Option<String> {
        let app = sb_app("com.apple.Music")?;
        if !is_playing(&app) {
            return None;
        }
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            eprintln!(
                "DEBUG now_playing: Apple Music currentTrack = {:?}",
                track.is_some()
            );
            let track = track?;
            let name: Option<Retained<NSString>> = msg_send![&*track, name];
            let result = name.map(|s| s.to_string());
            debug_log!("DEBUG now_playing: Apple Music title = {:?}", result);
            result
        }
    }

    fn sb_apple_music_artwork() -> Option<Vec<u8>> {
        let app = sb_app("com.apple.Music")?;
        if !is_playing(&app) {
            return None;
        }
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;

            // Get the artworks SBElementArray from the track.
            let artworks: *mut AnyObject = msg_send![&*track, artworks];
            eprintln!(
                "DEBUG now_playing: artworks ptr null = {}",
                artworks.is_null()
            );
            if !artworks.is_null() {
                // Don't trust count — go straight to objectAtIndex:0.
                // SBElementArray sends a targeted Apple Event for the specific
                // element, which can succeed even when count reports 0.
                let artwork: *mut AnyObject = msg_send![artworks, objectAtIndex: 0usize];
                eprintln!(
                    "DEBUG now_playing: artwork[0] ptr null = {}",
                    artwork.is_null()
                );
                if !artwork.is_null() {
                    // Properties on SBObject return lazy proxies — call `get`
                    // to force the Apple Event and materialize the real object.

                    // Try rawData first
                    let raw_proxy: *mut AnyObject = msg_send![artwork, rawData];
                    if !raw_proxy.is_null() {
                        let raw: *mut AnyObject = msg_send![raw_proxy, get];
                        debug_log!("DEBUG now_playing: rawData.get null = {}", raw.is_null());
                        if !raw.is_null() {
                            let len: usize = msg_send![raw, length];
                            debug_log!("DEBUG now_playing: rawData length = {}", len);
                            if len > 0 {
                                let ptr: *const u8 = msg_send![raw, bytes];
                                if !ptr.is_null() {
                                    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                                    eprintln!(
                                        "DEBUG now_playing: rawData artwork {} bytes",
                                        bytes.len()
                                    );
                                    return Some(bytes);
                                }
                            }
                        }
                    }

                    // Try data property (MusicPicture)
                    let data_proxy: *mut AnyObject = msg_send![artwork, data];
                    if !data_proxy.is_null() {
                        let data: *mut AnyObject = msg_send![data_proxy, get];
                        debug_log!("DEBUG now_playing: data.get null = {}", data.is_null());
                        if !data.is_null() {
                            let len: usize = msg_send![data, length];
                            debug_log!("DEBUG now_playing: data length = {}", len);
                            if len > 0 {
                                let ptr: *const u8 = msg_send![data, bytes];
                                if !ptr.is_null() {
                                    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                                    eprintln!(
                                        "DEBUG now_playing: data artwork {} bytes",
                                        bytes.len()
                                    );
                                    return Some(bytes);
                                }
                            }
                        }
                    }
                }
            }

            // iTunes Search API using artist + album.
            debug_log!("DEBUG now_playing: falling back to iTunes Search API");
            let name: Option<Retained<NSString>> = msg_send![&*track, name];
            let artist: Option<Retained<NSString>> = msg_send![&*track, artist];
            let album: Option<Retained<NSString>> = msg_send![&*track, album];
            let query = match (&artist, &album, &name) {
                (Some(a), Some(al), _) => format!("{} {}", a, al),
                (Some(a), None, Some(n)) => format!("{} {}", a, n),
                (_, _, Some(n)) => n.to_string(),
                _ => return None,
            };
            debug_log!("DEBUG now_playing: iTunes Search API query = {:?}", query);
            itunes_search_artwork(&query)
        }
    }

    /// Look up album artwork via the public iTunes Search API.
    fn itunes_search_artwork(query: &str) -> Option<Vec<u8>> {
        let url = format!(
            "https://itunes.apple.com/search?term={}&media=music&limit=1",
            urlencoded(query)
        );
        let resp: serde_json::Value = reqwest::blocking::get(&url).ok()?.json().ok()?;
        let art_url = resp["results"][0]["artworkUrl100"].as_str()?;
        // Request a larger image (600x600 instead of 100x100)
        let art_url = art_url.replace("100x100", "600x600");
        debug_log!("DEBUG now_playing: iTunes artwork URL = {}", art_url);
        reqwest::blocking::get(&art_url)
            .ok()?
            .bytes()
            .ok()
            .map(|b| b.to_vec())
    }

    fn urlencoded(s: &str) -> String {
        let mut out = String::with_capacity(s.len() * 2);
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    out.push(b as char);
                }
                b' ' => out.push('+'),
                _ => {
                    out.push('%');
                    out.push_str(&format!("{:02X}", b));
                }
            }
        }
        out
    }

    pub fn extract_colors(image_bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
        use auto_palette::{ImageData, Palette};

        let img = image::load_from_memory(image_bytes).ok()?;
        let rgba = img.to_rgba8();
        let image_data = ImageData::new(rgba.width(), rgba.height(), rgba.as_raw()).ok()?;
        let palette: Palette<f64> = Palette::extract(&image_data).ok()?;
        let mut swatches = palette.swatches().to_vec();
        swatches.sort_by_key(|s| std::cmp::Reverse(s.population()));
        let colors: Vec<[u8; 3]> = swatches
            .iter()
            .filter(|s| s.color().to_oklch().l > 0.15)
            .take(4)
            .map(|s| {
                let rgb = s.color().to_rgb();
                [rgb.r, rgb.g, rgb.b]
            })
            .collect();
        if colors.is_empty() {
            None
        } else {
            Some(colors)
        }
    }
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    pub fn get_track_title() -> Option<String> {
        let output = std::process::Command::new("playerctl")
            .args(["metadata", "title"])
            .output()
            .ok()?;
        if output.status.success() {
            let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
        None
    }

    pub fn fetch_artwork_bytes() -> Option<Vec<u8>> {
        let output = std::process::Command::new("playerctl")
            .args(["metadata", "mpris:artUrl"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if url.is_empty() {
            return None;
        }
        if url.starts_with("file://") {
            std::fs::read(url.trim_start_matches("file://")).ok()
        } else {
            reqwest::blocking::get(&url)
                .ok()?
                .bytes()
                .ok()
                .map(|b| b.to_vec())
        }
    }

    pub fn extract_colors_from_bytes(bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
        use auto_palette::{ImageData, Palette};

        let img = image::load_from_memory(bytes).ok()?;
        let rgba = img.to_rgba8();
        let image_data = ImageData::new(rgba.width(), rgba.height(), rgba.as_raw()).ok()?;
        let palette: Palette<f64> = Palette::extract(&image_data).ok()?;
        let mut swatches = palette.swatches().to_vec();
        swatches.sort_by_key(|s| std::cmp::Reverse(s.population()));
        let colors: Vec<[u8; 3]> = swatches
            .iter()
            .filter(|s| s.color().to_oklch().l > 0.15)
            .take(4)
            .map(|s| {
                let rgb = s.color().to_rgb();
                [rgb.r, rgb.g, rgb.b]
            })
            .collect();
        if colors.is_empty() {
            None
        } else {
            Some(colors)
        }
    }
}
