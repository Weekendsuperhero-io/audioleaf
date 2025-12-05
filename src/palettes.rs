/// Predefined color palettes
///
/// This module contains named color palettes that users can reference
/// in their config.toml instead of manually specifying hue arrays.
use std::collections::HashMap;

/// Get a predefined palette by name
pub fn get_palette(name: &str) -> Option<Vec<u16>> {
    let palettes = get_all_palettes();
    palettes.get(name).cloned()
}

/// Get all available palette names
pub fn get_palette_names() -> Vec<String> {
    get_all_palettes().keys().cloned().collect()
}

/// Get all predefined palettes
fn get_all_palettes() -> HashMap<String, Vec<u16>> {
    let mut palettes = HashMap::new();

    palettes.insert(
        "ocean-nightclub".to_string(),
        vec![195, 210, 240, 270, 285, 300, 180],
    );

    palettes.insert(
        "sunset".to_string(),
        vec![15, 25, 340, 350, 0, 10, 310, 280],
    );

    palettes.insert(
        "house-music-party".to_string(),
        vec![300, 285, 270, 255, 240, 195, 180],
    );

    palettes.insert(
        "tropical-beach".to_string(),
        vec![180, 175, 170, 160, 90, 75, 60],
    );

    palettes.insert("fire".to_string(), vec![0, 10, 20, 30, 40, 50, 60]);

    palettes.insert("forest".to_string(), vec![90, 100, 110, 120, 130, 140, 150]);

    palettes.insert("neon-rainbow".to_string(), vec![0, 60, 120, 180, 240, 300]);

    palettes.insert(
        "pink-dreams".to_string(),
        vec![320, 325, 330, 335, 340, 345, 350],
    );

    palettes.insert(
        "cool-blues".to_string(),
        vec![190, 200, 210, 220, 230, 240, 250],
    );

    palettes.insert(
        "tmnt".to_string(),
        vec![125, 130, 240, 245, 25, 30, 0, 5, 280, 285],
    );

    palettes.insert("christmas".to_string(), vec![360, 0, 120, 5, 125, 5, 360]);

    palettes
}
