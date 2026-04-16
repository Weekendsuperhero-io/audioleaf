/// Predefined color palettes
///
/// This module contains named color palettes that users can reference
/// in their config.toml instead of manually specifying RGB color arrays.
/// Each color is an [R, G, B] triplet. At runtime these are converted
/// to Oklch so the visualizer can animate perceptually-uniform lightness.
use hashbrown::HashMap;

/// Get a predefined palette by name
pub fn get_palette(name: &str) -> Option<Vec<[u8; 3]>> {
    let palettes = get_all_palettes();
    palettes.get(name).cloned()
}

/// Get all available palette names
pub fn get_palette_names() -> Vec<String> {
    get_all_palettes().keys().cloned().collect()
}

/// Get all predefined palettes
fn get_all_palettes() -> HashMap<String, Vec<[u8; 3]>> {
    let mut palettes = HashMap::new();

    // Deep sky blues, indigos, magentas, and cyans
    palettes.insert(
        "ocean-nightclub".to_string(),
        vec![
            [0, 191, 255], // deep sky blue
            [0, 128, 255], // azure
            [0, 0, 255],   // blue
            [128, 0, 255], // violet
            [191, 0, 255], // purple
            [255, 0, 255], // magenta
            [0, 255, 255], // cyan
        ],
    );

    // Warm reds, oranges, deep pinks, and purples
    palettes.insert(
        "sunset".to_string(),
        vec![
            [255, 64, 0],  // red-orange
            [255, 106, 0], // orange
            [255, 0, 85],  // crimson
            [255, 0, 43],  // rose
            [255, 0, 0],   // red
            [255, 43, 0],  // scarlet
            [255, 0, 128], // hot pink
            [170, 0, 255], // violet
        ],
    );

    // Magentas, purples, indigos, and cyans
    palettes.insert(
        "house-music-party".to_string(),
        vec![
            [255, 0, 255], // magenta
            [191, 0, 255], // purple
            [128, 0, 255], // violet
            [64, 0, 255],  // indigo
            [0, 0, 255],   // blue
            [0, 191, 255], // deep sky blue
            [0, 255, 255], // cyan
        ],
    );

    // Cyans, teals, greens, and limes
    palettes.insert(
        "tropical-beach".to_string(),
        vec![
            [0, 255, 255], // cyan
            [0, 255, 234], // turquoise
            [0, 255, 213], // aquamarine
            [0, 255, 170], // spring green
            [128, 255, 0], // lime
            [191, 255, 0], // chartreuse
            [255, 255, 0], // yellow
        ],
    );

    // Reds through oranges to yellow
    palettes.insert(
        "fire".to_string(),
        vec![
            [255, 0, 0],   // red
            [255, 43, 0],  // scarlet
            [255, 85, 0],  // vermillion
            [255, 128, 0], // orange
            [255, 170, 0], // amber
            [255, 213, 0], // golden
            [255, 255, 0], // yellow
        ],
    );

    // Chartreuse through greens to teal
    palettes.insert(
        "forest".to_string(),
        vec![
            [128, 255, 0], // lime
            [85, 255, 0],  // green-yellow
            [43, 255, 0],  // lawn green
            [0, 255, 0],   // green
            [0, 255, 43],  // emerald
            [0, 255, 85],  // jade
            [0, 255, 128], // mint
        ],
    );

    // Full spectrum rainbow
    palettes.insert(
        "neon-rainbow".to_string(),
        vec![
            [255, 0, 0],   // red
            [255, 255, 0], // yellow
            [0, 255, 0],   // green
            [0, 255, 255], // cyan
            [0, 0, 255],   // blue
            [255, 0, 255], // magenta
        ],
    );

    // Pinks from deep to light rose
    palettes.insert(
        "pink-dreams".to_string(),
        vec![
            [255, 0, 170], // deep pink
            [255, 0, 149], // hot pink
            [255, 0, 128], // pink
            [255, 0, 106], // rose
            [255, 0, 85],  // fuchsia-rose
            [255, 0, 64],  // crimson-rose
            [255, 0, 43],  // warm rose
        ],
    );

    // Cool blue spectrum
    palettes.insert(
        "cool-blues".to_string(),
        vec![
            [0, 170, 255], // sky blue
            [0, 128, 255], // azure
            [0, 85, 255],  // royal blue
            [0, 43, 255],  // cobalt
            [0, 0, 255],   // blue
            [43, 0, 255],  // indigo
            [64, 0, 255],  // deep indigo
        ],
    );

    // Teenage Mutant Ninja Turtles: character bandana colors + turtle green
    palettes.insert(
        "tmnt".to_string(),
        vec![
            [0, 200, 0],   // turtle green
            [0, 80, 255],  // Leonardo blue
            [128, 0, 255], // Donatello purple
            [255, 128, 0], // Michelangelo orange
            [255, 0, 0],   // Raphael red
            [0, 255, 64],  // sewer green
            [0, 128, 255], // Leonardo azure
            [170, 0, 255], // Donatello violet
            [255, 170, 0], // Michelangelo amber
            [255, 43, 0],  // Raphael scarlet
        ],
    );

    // Red, white, and green
    palettes.insert(
        "christmas".to_string(),
        vec![
            [255, 0, 0],     // red
            [255, 0, 0],     // red
            [0, 255, 0],     // green
            [255, 21, 0],    // warm red
            [0, 255, 43],    // festive green
            [255, 21, 0],    // warm red
            [255, 255, 255], // white
        ],
    );

    palettes
}
