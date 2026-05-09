//! This module previously held a static catalog of named palettes that users
//! referenced from `config.toml` via `colors = "ocean-nightclub"`. As of the
//! Nanoleaf-sourced palette refactor, palettes are pulled live from the
//! connected device's saved effects (see `NlDevice::list_effect_palettes`).
//!
//! The static catalog is gone. Re-export the palette type so callers don't
//! need to know that the source moved to `nanoleaf.rs`.

pub use crate::nanoleaf::NamedPalette;
