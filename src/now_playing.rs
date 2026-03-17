/// Album art color extraction for the visualizer.
///
/// macOS: Uses ScriptingBridge.framework (via objc2) to query running media
///   players directly through the ObjC bridge — no subprocess spawning.
///   - Spotify: title, artist, artwork URL (downloaded via reqwest).
///   - Apple Music: title, artist, raw artwork bytes from MusicArtwork.rawData.
///   Falls back to osascript if ScriptingBridge fails.
///
/// Linux: Uses `playerctl` subprocess.

/// Returns the title of the currently playing track.
pub fn get_track_title() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::get_track_info().map(|info| info.title)
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

/// Returns dominant RGB colors extracted from the current track's album artwork.
pub fn fetch_palette() -> Option<Vec<[u8; 3]>> {
    #[cfg(target_os = "macos")]
    {
        macos::fetch_palette()
    }
    #[cfg(target_os = "linux")]
    {
        linux::fetch_palette()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

// ── macOS — ScriptingBridge + osascript fallback ──────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSData, NSString};

    #[link(name = "ScriptingBridge", kind = "framework")]
    unsafe extern "C" {}

    // Four-char code for "playing" state (shared by Spotify and Apple Music).
    const EPLAYER_PLAYING: u32 = 0x6b505350; // 'kPSP'

    pub struct TrackInfo {
        pub title: String,
    }

    pub fn get_track_info() -> Option<TrackInfo> {
        sb_spotify_title()
            .or_else(sb_apple_music_title)
            .or_else(osascript_title)
            .map(|title| TrackInfo { title })
    }

    pub fn fetch_palette() -> Option<Vec<[u8; 3]>> {
        let bytes = sb_spotify_artwork()
            .or_else(sb_apple_music_artwork)
            .or_else(osascript_artwork)?;
        extract_colors(&bytes)
    }

    // ── ScriptingBridge helpers ───────────────────────────────────────────────

    /// Open a scripting bridge to a running application. Returns None if not running.
    fn sb_app(bundle_id: &str) -> Option<Retained<AnyObject>> {
        unsafe {
            let cls = AnyClass::get(c"SBApplication")?;
            let bid = NSString::from_str(bundle_id);
            let app: Option<Retained<AnyObject>> =
                msg_send![cls, applicationWithBundleIdentifier: &*bid];
            let app = app?;
            let running: bool = msg_send![&*app, isRunning];
            if !running {
                return None;
            }
            let state: u32 = msg_send![&*app, playerState];
            if state != EPLAYER_PLAYING {
                return None;
            }
            Some(app)
        }
    }

    // ── Spotify via ScriptingBridge ───────────────────────────────────────────

    fn sb_spotify_title() -> Option<String> {
        let app = sb_app("com.spotify.client")?;
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;
            let name: Option<Retained<NSString>> = msg_send![&*track, name];
            name.map(|s| s.to_string())
        }
    }

    fn sb_spotify_artwork() -> Option<Vec<u8>> {
        let app = sb_app("com.spotify.client")?;
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;
            let url: Option<Retained<NSString>> = msg_send![&*track, artworkUrl];
            let url = url?;
            reqwest::blocking::get(&*url.to_string())
                .ok()?
                .bytes()
                .ok()
                .map(|b| b.to_vec())
        }
    }

    // ── Apple Music via ScriptingBridge ───────────────────────────────────────

    fn sb_apple_music_title() -> Option<String> {
        let app = sb_app("com.apple.Music")?;
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;
            let name: Option<Retained<NSString>> = msg_send![&*track, name];
            name.map(|s| s.to_string())
        }
    }

    fn sb_apple_music_artwork() -> Option<Vec<u8>> {
        let app = sb_app("com.apple.Music")?;
        unsafe {
            let track: Option<Retained<AnyObject>> = msg_send![&*app, currentTrack];
            let track = track?;
            let artworks: Option<Retained<AnyObject>> = msg_send![&*track, artworks];
            let artworks = artworks?;
            let count: usize = msg_send![&*artworks, count];
            if count == 0 {
                return None;
            }
            let artwork: Option<Retained<AnyObject>> = msg_send![&*artworks, objectAtIndex: 0usize];
            let artwork = artwork?;
            let raw: Option<Retained<AnyObject>> = msg_send![&*artwork, rawData];
            let raw = raw?;
            // rawData returns NSData with JPEG/PNG image bytes.
            raw.downcast_ref::<NSData>().map(|d| d.to_vec())
        }
    }

    // ── osascript fallback ───────────────────────────────────────────────────

    fn osascript_title() -> Option<String> {
        for script in [
            r#"tell application "System Events"
                if not (exists process "Spotify") then return "NOT_RUNNING"
            end tell
            tell application "Spotify"
                if player state is not playing then return "NOT_PLAYING"
                return name of current track
            end tell"#,
            r#"tell application "System Events"
                if not (exists process "Music") then return "NOT_RUNNING"
            end tell
            tell application "Music"
                if player state is not playing then return "NOT_PLAYING"
                return name of current track
            end tell"#,
        ] {
            if let Some(title) = run_osascript(script) {
                return Some(title);
            }
        }
        None
    }

    fn osascript_artwork() -> Option<Vec<u8>> {
        // Spotify: artwork URL.
        let script = r#"tell application "System Events"
            if not (exists process "Spotify") then return "NOT_RUNNING"
        end tell
        tell application "Spotify"
            if player state is not playing then return "NOT_PLAYING"
            return artwork url of current track
        end tell"#;
        if let Some(url) = run_osascript(script) {
            if let Some(bytes) = reqwest::blocking::get(&url)
                .ok()
                .and_then(|r| r.bytes().ok())
                .map(|b| b.to_vec())
            {
                return Some(bytes);
            }
        }
        // Apple Music: use iTunes Search API.
        let script = r#"tell application "System Events"
            if not (exists process "Music") then return "NOT_RUNNING"
        end tell
        tell application "Music"
            if player state is not playing then return "NOT_PLAYING"
            return (name of current track) & " " & (artist of current track)
        end tell"#;
        if let Some(query) = run_osascript(script) {
            if let Some(url) = itunes_artwork_url(&query) {
                return reqwest::blocking::get(&url)
                    .ok()
                    .and_then(|r| r.bytes().ok())
                    .map(|b| b.to_vec());
            }
        }
        None
    }

    fn run_osascript(script: &str) -> Option<String> {
        let output = std::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() || text == "NOT_RUNNING" || text == "NOT_PLAYING" {
            return None;
        }
        Some(text)
    }

    fn itunes_artwork_url(query: &str) -> Option<String> {
        let url = format!(
            "https://itunes.apple.com/search?term={}&media=music&limit=1",
            urlencoded(query)
        );
        let resp: serde_json::Value = reqwest::blocking::get(&url).ok()?.json().ok()?;
        let art = resp["results"][0]["artworkUrl100"].as_str()?;
        Some(art.replace("100x100", "600x600"))
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

    fn extract_colors(image_bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
        use auto_palette::{ImageData, Palette};

        let img = image::load_from_memory(image_bytes).ok()?;
        let rgba = img.to_rgba8();
        let image_data = ImageData::new(rgba.width(), rgba.height(), rgba.as_raw()).ok()?;
        let palette: Palette<f64> = Palette::extract(&image_data).ok()?;
        // Sort by population (most prevalent colors first) and take the
        // top 4 that aren't near-black — these are the actual dominant
        // colors of the album art, not theme-scored "interesting" picks.
        let mut swatches = palette.swatches().to_vec();
        swatches.sort_by(|a, b| b.population().cmp(&a.population()));
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

    pub fn fetch_palette() -> Option<Vec<[u8; 3]>> {
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

        let bytes: Vec<u8> = if url.starts_with("file://") {
            std::fs::read(url.trim_start_matches("file://")).ok()?
        } else {
            reqwest::blocking::get(&url).ok()?.bytes().ok()?.to_vec()
        };

        use auto_palette::{ImageData, Palette};

        let img = image::load_from_memory(&bytes).ok()?;
        let rgba = img.to_rgba8();
        let image_data = ImageData::new(rgba.width(), rgba.height(), rgba.as_raw()).ok()?;
        let palette: Palette<f64> = Palette::extract(&image_data).ok()?;
        let mut swatches = palette.swatches().to_vec();
        swatches.sort_by(|a, b| b.population().cmp(&a.population()));
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
