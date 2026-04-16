use crate::audio;
use crate::constants;
use crate::layout_visualizer::PanelInfo;
use crate::visualizer::{self, VisualizerMsg};
use crate::{
    config::{Axis, Effect, Sort, VisualizerConfig},
    nanoleaf::NlDevice,
};
use anyhow::Result;
use hashbrown::HashMap;
use macroquad::prelude::*;
use parking_lot::Mutex;
use std::f32::consts::PI;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

/// Display state shared between the main thread and the album art watcher thread.
struct VizState {
    colors: Vec<[u8; 3]>,
    track_title: Option<String>,
    artwork_bytes: Option<Vec<u8>>,
    /// Incremented each time artwork_bytes changes, so the UI can detect updates
    /// without cloning the bytes every frame.
    artwork_generation: u64,
}

pub struct App {
    // Device
    nl_device: NlDevice,
    panels: Vec<PanelInfo>,
    global_orientation: u16,

    // Visualizer
    visualizer_tx: mpsc::Sender<VisualizerMsg>,
    shared_colors: Arc<Mutex<HashMap<u16, [u8; 3]>>>,

    // Settings
    gain: f32,
    time_window: f32,
    transition_time: u16,
    min_freq: u16,
    max_freq: u16,
    freq_preset_index: usize,
    current_palette_index: usize,
    palette_names: Vec<String>,
    effect: Effect,
    primary_axis: Axis,
    sort_primary: Sort,
    sort_secondary: Sort,

    // UI state
    show_visualization: bool,
    show_help: bool,

    // Color interpolation for smooth panel preview
    prev_colors: HashMap<u16, [u8; 3]>,
    target_colors: HashMap<u16, [u8; 3]>,
    color_transition_start: Instant,

    // Album art
    viz_state: Arc<Mutex<VizState>>,
    album_art_stop: Option<Arc<AtomicBool>>,
    album_art_texture: Option<Texture2D>,
    /// Tracks which artwork generation we've already loaded into the texture
    loaded_artwork_gen: u64,
}

impl App {
    /// Constructs a new `App` with macroquad-based graphical UI.
    ///
    /// Initializes the audio visualizer thread, fetches panel layout from the device,
    /// and prepares all settings state. Requests UDP control immediately.
    pub fn new(nl_device: NlDevice, visualizer_config: VisualizerConfig) -> Result<Self> {
        let audio_stream = audio::AudioStream::new(visualizer_config.audio_backend.as_deref())?;
        let gain = visualizer_config
            .default_gain
            .unwrap_or(constants::DEFAULT_GAIN);
        #[cfg(debug_assertions)]
        eprintln!("INFO: Starting with gain: {}", gain);

        let global_orientation = nl_device
            .get_global_orientation()
            .ok()
            .and_then(|o| o["value"].as_u64())
            .unwrap_or(0) as u16;

        let primary_axis = visualizer_config.primary_axis.unwrap_or_default();
        let sort_primary = visualizer_config.sort_primary.unwrap_or_default();
        let sort_secondary = visualizer_config.sort_secondary.unwrap_or_default();
        let effect = visualizer_config.effect.unwrap_or_default();
        let (min_freq, max_freq) = visualizer_config
            .freq_range
            .unwrap_or(constants::DEFAULT_FREQ_RANGE);
        let time_window = visualizer_config
            .time_window
            .unwrap_or(constants::DEFAULT_TIME_WINDOW);
        let transition_time = visualizer_config
            .transition_time
            .unwrap_or(constants::DEFAULT_TRANSITION_TIME);

        let initial_colors = visualizer_config
            .colors
            .clone()
            .unwrap_or_else(|| Vec::from(constants::DEFAULT_COLORS));

        // Fetch panel layout for graphical rendering
        let layout = nl_device.get_panel_layout()?;
        let panels = crate::layout_visualizer::parse_layout(&layout)?;

        // Shared color state: visualizer thread writes panel colors, UI reads them
        let shared_colors: Arc<Mutex<HashMap<u16, [u8; 3]>>> = Arc::new(Mutex::new(HashMap::new()));

        let tx = visualizer::Visualizer::new(
            visualizer_config,
            audio_stream,
            &nl_device,
            Arc::clone(&shared_colors),
        )?
        .with_stream_health(Arc::new(Mutex::new(visualizer::StreamHealth::Starting)))
        .init();
        // Keep VisualizerMsg::Ping live across all binaries; no-op for the visualizer state machine.
        let _ = tx.send(visualizer::VisualizerMsg::Ping);

        let mut palette_names = crate::palettes::get_palette_names();
        palette_names.sort();

        let viz_state = Arc::new(Mutex::new(VizState {
            colors: initial_colors,
            track_title: None,
            artwork_bytes: None,
            artwork_generation: 0,
        }));

        nl_device.request_udp_control()?;

        Ok(App {
            nl_device,
            panels,
            global_orientation,
            visualizer_tx: tx,
            shared_colors,
            gain,
            time_window,
            transition_time,
            min_freq,
            max_freq,
            freq_preset_index: constants::FREQ_RANGE_PRESETS
                .iter()
                .position(|&(lo, hi, _)| lo == min_freq && hi == max_freq)
                .unwrap_or(0),
            current_palette_index: 0,
            palette_names,
            effect,
            primary_axis,
            sort_primary,
            sort_secondary,
            show_visualization: false,
            show_help: false,
            prev_colors: HashMap::new(),
            target_colors: HashMap::new(),
            color_transition_start: Instant::now(),
            viz_state,
            album_art_stop: None,
            album_art_texture: None,
            loaded_artwork_gen: 0,
        })
    }

    /// Launches the macroquad window and runs the main graphical event loop.
    /// Blocks until the window is closed.
    pub fn run(self) {
        let title = format!("Audioleaf - {}", self.nl_device.name);
        macroquad::Window::from_config(
            Conf {
                window_title: title,
                window_width: 1200,
                window_height: 800,
                window_resizable: true,
                icon: Some(load_icon()),
                ..Default::default()
            },
            async move {
                let mut app = self;
                app.main_loop().await;
            },
        );
    }

    async fn main_loop(&mut self) {
        loop {
            clear_background(Color::from_rgba(20, 20, 30, 255));

            if self.handle_input() {
                break;
            }

            // Check if artwork bytes have changed and reload texture
            self.update_album_art_texture();

            self.draw_panels();
            self.draw_hud();

            if self.show_help {
                self.draw_help_overlay();
            }

            next_frame().await;
        }

        // Shutdown
        let _ = self.visualizer_tx.send(VisualizerMsg::End);
        self.stop_album_art_watcher();
        let _ = self.nl_device.set_state(Some(false), Some(0));
    }

    /// Process keyboard input. Returns true if quit was requested.
    fn handle_input(&mut self) -> bool {
        if is_key_pressed(KeyCode::Escape) {
            return true;
        }

        while let Some(ch) = get_char_pressed() {
            match ch {
                'Q' => return true,
                '?' => self.show_help = !self.show_help,
                _ if self.show_help => {} // Swallow other keys while help is shown
                '-' | '_' => {
                    self.transition_time = self.transition_time.saturating_sub(1);
                    let _ = self
                        .visualizer_tx
                        .send(VisualizerMsg::SetTransitionTime(self.transition_time));
                }
                '=' | '+' => {
                    self.transition_time = (self.transition_time + 1).min(10);
                    let _ = self
                        .visualizer_tx
                        .send(VisualizerMsg::SetTransitionTime(self.transition_time));
                }
                'g' => {
                    self.gain -= 0.05;
                    let _ = self.visualizer_tx.send(VisualizerMsg::SetGain(self.gain));
                }
                'G' => {
                    self.gain += 0.05;
                    let _ = self.visualizer_tx.send(VisualizerMsg::SetGain(self.gain));
                }
                '1'..='9' => {
                    let index = (ch as usize) - ('1' as usize);
                    self.change_palette(index);
                }
                '0' => self.change_palette(9),
                'a' | 'A' => self.toggle_axis(),
                'p' | 'P' => self.toggle_primary_sort(),
                's' | 'S' => self.toggle_secondary_sort(),
                'e' | 'E' => self.cycle_effect(),
                'n' | 'N' => self.use_album_art_palette(),
                'r' | 'R' => {
                    let _ = self.visualizer_tx.send(VisualizerMsg::ResetPanels);
                }
                '[' => {
                    self.time_window = (self.time_window - 0.025).max(0.05);
                    let _ = self
                        .visualizer_tx
                        .send(VisualizerMsg::SetTimeWindow(self.time_window));
                }
                ']' => {
                    self.time_window = (self.time_window + 0.025).min(0.5);
                    let _ = self
                        .visualizer_tx
                        .send(VisualizerMsg::SetTimeWindow(self.time_window));
                }
                'f' | 'F' => {
                    self.freq_preset_index =
                        (self.freq_preset_index + 1) % constants::FREQ_RANGE_PRESETS.len();
                    let (lo, hi, _) = constants::FREQ_RANGE_PRESETS[self.freq_preset_index];
                    self.min_freq = lo;
                    self.max_freq = hi;
                    let _ = self.visualizer_tx.send(VisualizerMsg::SetFreqRange(lo, hi));
                }
                ' ' => self.show_visualization = !self.show_visualization,
                _ => {}
            }
        }

        false
    }

    // ── Color interpolation ─────────────────────────────────────────────

    fn interpolated_colors(&self) -> HashMap<u16, [u8; 3]> {
        // transition_time is in units of 100ms
        let duration = Duration::from_millis(self.transition_time as u64 * 100);
        let elapsed = self.color_transition_start.elapsed();
        let t = if duration.is_zero() {
            1.0_f32
        } else {
            (elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0)
        };

        let mut result = self.target_colors.clone();
        if t < 1.0 {
            for (id, target) in &self.target_colors {
                let prev = self.prev_colors.get(id).copied().unwrap_or([0, 0, 0]);
                result.insert(
                    *id,
                    [
                        lerp_u8(prev[0], target[0], t),
                        lerp_u8(prev[1], target[1], t),
                        lerp_u8(prev[2], target[2], t),
                    ],
                );
            }
        }
        result
    }

    // ── Panel rendering ──────────────────────────────────────────────────

    fn draw_panels(&mut self) {
        let sw = screen_width();
        let sh = screen_height();

        // Find the largest panel radius (in layout units) so we can include
        // the full extent of edge panels in the bounding box, not just centers.
        let max_panel_radius = self
            .panels
            .iter()
            .filter(|p| p.shape_type.side_length >= 1.0)
            .map(|p| {
                let s = p.shape_type.side_length;
                match p.shape_type.num_sides() {
                    3 => s / f32::sqrt(3.0),
                    4 => s / f32::sqrt(2.0),
                    _ => s,
                }
            })
            .fold(0.0_f32, f32::max);

        let min_x = self.panels.iter().map(|p| p.x).min().unwrap_or(0) as f32 - max_panel_radius;
        let max_x = self.panels.iter().map(|p| p.x).max().unwrap_or(0) as f32 + max_panel_radius;
        let min_y = self.panels.iter().map(|p| p.y).min().unwrap_or(0) as f32 - max_panel_radius;
        let max_y = self.panels.iter().map(|p| p.y).max().unwrap_or(0) as f32 + max_panel_radius;

        let layout_width = (max_x - min_x).max(1.0);
        let layout_height = (max_y - min_y).max(1.0);

        let padding_top = 40.0;
        let padding_bottom = 40.0;
        let padding_sides = 40.0;
        let available_width = sw - 2.0 * padding_sides;
        let available_height = sh - padding_top - padding_bottom;

        let scale = (available_width / layout_width).min(available_height / layout_height);

        let offset_x = (sw - layout_width * scale) / 2.0;
        let offset_y = padding_top + (available_height - layout_height * scale) / 2.0;

        // Snapshot visualization colors with smooth interpolation
        let vis_colors = if self.show_visualization {
            let map = self.shared_colors.lock();
            if *map != self.target_colors {
                self.prev_colors = self.interpolated_colors();
                self.target_colors = map.clone();
                self.color_transition_start = Instant::now();
            }
            Some(self.interpolated_colors())
        } else {
            None
        };

        // First pass: compute rotated screen positions
        let transformed: Vec<(f32, f32)> = self
            .panels
            .iter()
            .map(|panel| {
                let rel_x = (panel.x as f32 - min_x) - layout_width / 2.0;
                let rel_y = (panel.y as f32 - min_y) - layout_height / 2.0;
                let angle = -(self.global_orientation as f32).to_radians();
                let rotated_x = rel_x * angle.cos() - rel_y * angle.sin();
                let rotated_y = rel_x * angle.sin() + rel_y * angle.cos();
                let screen_x = offset_x + (rotated_x + layout_width / 2.0) * scale;
                let screen_y = offset_y + (layout_height / 2.0 - rotated_y) * scale;
                (screen_x, screen_y)
            })
            .collect();

        // Second pass: draw
        for (i, panel) in self.panels.iter().enumerate() {
            let (x, y) = transformed[i];
            if panel.shape_type.side_length < 1.0 {
                draw_controller(x, y, panel, scale, &self.panels, &transformed);
            } else {
                self.draw_light_panel(x, y, panel, scale, &vis_colors);
            }
        }
    }

    fn draw_light_panel(
        &self,
        x: f32,
        y: f32,
        panel: &PanelInfo,
        scale: f32,
        vis_colors: &Option<HashMap<u16, [u8; 3]>>,
    ) {
        let num_sides = panel.shape_type.num_sides();
        let side_length = panel.shape_type.side_length * scale;

        let radius = match num_sides {
            3 => side_length / f32::sqrt(3.0),
            4 => side_length / f32::sqrt(2.0),
            _ => side_length,
        };

        let start_angle = (panel.orientation as f32).to_radians();
        let vertices: Vec<Vec2> = (0..num_sides)
            .map(|i| {
                let angle = start_angle + (i as f32 * 2.0 * PI / num_sides as f32);
                Vec2::new(x + radius * angle.cos(), y + radius * angle.sin())
            })
            .collect();

        // Determine fill color: live visualization or static shape-type color
        let color = if let Some(colors_map) = vis_colors {
            if let Some(&[r, g, b]) = colors_map.get(&panel.panel_id) {
                Color::from_rgba(r, g, b, 255)
            } else {
                Color::from_rgba(30, 30, 40, 200)
            }
        } else {
            match panel.shape_type.id {
                0 | 8 | 9 => Color::from_rgba(255, 100, 100, 200),
                2..=4 => Color::from_rgba(100, 255, 100, 200),
                7 | 14 | 15 => Color::from_rgba(100, 150, 255, 200),
                30..=32 => Color::from_rgba(255, 255, 100, 200),
                _ => Color::from_rgba(150, 150, 150, 200),
            }
        };

        // Fill polygon (triangle fan)
        for i in 1..(num_sides - 1) {
            draw_triangle(vertices[0], vertices[i], vertices[i + 1], color);
        }

        // Outline
        let outline = if vis_colors.is_some() {
            Color::from_rgba(60, 60, 80, 255)
        } else {
            WHITE
        };
        for i in 0..num_sides {
            let next = (i + 1) % num_sides;
            draw_line(
                vertices[i].x,
                vertices[i].y,
                vertices[next].x,
                vertices[next].y,
                2.0,
                outline,
            );
        }
    }

    // ── Album art texture ─────────────────────────────────────────────────

    fn update_album_art_texture(&mut self) {
        // Check the generation counter without cloning the (potentially large) bytes.
        // Only lock briefly to read the generation and, if changed, take the bytes.
        let update = {
            let s = self.viz_state.lock();
            if s.artwork_bytes.is_some() && s.artwork_generation != self.loaded_artwork_gen {
                // Only clone when the artwork actually changed (not every frame)
                Some((s.artwork_bytes.clone(), s.artwork_generation))
            } else if s.artwork_bytes.is_none() && self.album_art_texture.is_some() {
                Some((None, s.artwork_generation))
            } else {
                None
            }
        };

        if let Some((bytes_opt, generation)) = update {
            if let Some(bytes) = bytes_opt {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = (rgba.width() as u16, rgba.height() as u16);
                    let tex = Texture2D::from_rgba8(w, h, rgba.as_raw());
                    tex.set_filter(FilterMode::Linear);
                    // Free the old GPU texture before replacing
                    if let Some(old) = self.album_art_texture.take() {
                        drop(old);
                    }
                    self.album_art_texture = Some(tex);
                }
            } else {
                // Artwork cleared (switched to named palette)
                if let Some(old) = self.album_art_texture.take() {
                    drop(old);
                }
            }
            self.loaded_artwork_gen = generation;
        }
    }

    // ── HUD ──────────────────────────────────────────────────────────────

    fn draw_hud(&self) {
        let sw = screen_width();
        let sh = screen_height();

        // ── Top-left: device name ──
        let name_text = format!("Connected to {}", self.nl_device.name);
        let name_size = 28.0;
        let nm = sharp_measure(name_size, &name_text);
        draw_rectangle(
            5.0,
            2.0,
            nm.width + 14.0,
            nm.height + 10.0,
            Color::from_rgba(0, 0, 0, 160),
        );
        sharp_text(
            &name_text,
            12.0,
            nm.height + 5.0,
            name_size,
            Color::from_rgba(220, 130, 255, 255),
        );

        // ── Top-right: preview toggle ──
        let vis_text = if self.show_visualization {
            "Preview: ON [Space]"
        } else {
            "Preview: OFF [Space]"
        };
        let vis_color = if self.show_visualization {
            Color::from_rgba(100, 255, 100, 255)
        } else {
            Color::from_rgba(150, 150, 150, 255)
        };
        let vm = sharp_measure(22.0, vis_text);
        draw_rectangle(
            sw - vm.width - 19.0,
            2.0,
            vm.width + 14.0,
            vm.height + 10.0,
            Color::from_rgba(0, 0, 0, 160),
        );
        sharp_text(
            vis_text,
            sw - vm.width - 12.0,
            vm.height + 5.0,
            22.0,
            vis_color,
        );

        // ── Bottom-left: effect + palette + colors ──
        let effect_str = match self.effect {
            Effect::Spectrum => "Spectrum",
            Effect::EnergyWave => "Energy Wave",
            Effect::Pulse => "Pulse",
        };
        let (palette_name, track_title) = {
            let state = self.viz_state.lock();
            let name = if state.track_title.is_some() {
                "album-art".to_string()
            } else if self.current_palette_index < self.palette_names.len() {
                self.palette_names[self.current_palette_index].clone()
            } else {
                "Unknown".to_string()
            };
            (name, state.track_title.clone())
        };

        let freq_label = constants::FREQ_RANGE_PRESETS
            .iter()
            .find(|&&(lo, hi, _)| lo == self.min_freq && hi == self.max_freq)
            .map(|&(_, _, name)| name)
            .unwrap_or("Custom");
        let effect_text = format!("Effect: {}  |  Gain: {:.2}", effect_str, self.gain);
        let audio_text = format!(
            "Window: {:.0}ms  |  Transition: {}ms  |  Freq: {} ({}-{}Hz)",
            self.time_window * 1000.0,
            self.transition_time as u32 * 100,
            freq_label,
            self.min_freq,
            self.max_freq,
        );
        let mut palette_text = format!("Palette: {}", palette_name);
        if let Some(title) = &track_title {
            palette_text.push_str(&format!("  |  Now playing: {}", title));
        }

        let big = 26.0;
        let small = 20.0;
        let em = sharp_measure(big, &effect_text);
        let am = sharp_measure(small, &audio_text);
        let pm = sharp_measure(big, &palette_text);
        let box_w = em.width.max(pm.width).max(am.width) + 14.0;

        // Color swatches measurement
        let colors = self.viz_state.lock().colors.clone();
        let swatch_total_w = 22.0 * colors.len() as f32;
        let box_w = box_w.max(swatch_total_w + 80.0);

        let box_h = big * 2.0 + small + 34.0 + 18.0; // three text lines + swatch row + padding
        let box_y = sh - box_h - 5.0;

        draw_rectangle(5.0, box_y, box_w, box_h, Color::from_rgba(0, 0, 0, 160));

        let line1_y = box_y + big + 4.0;
        sharp_text(&effect_text, 12.0, line1_y, big, WHITE);

        let line2_y = line1_y + small + 4.0;
        sharp_text(
            &audio_text,
            12.0,
            line2_y,
            small,
            Color::from_rgba(180, 180, 220, 255),
        );

        let line3_y = line2_y + big + 4.0;
        sharp_text(
            &palette_text,
            12.0,
            line3_y,
            big,
            Color::from_rgba(100, 255, 100, 255),
        );

        // Color swatches
        let swatch_y = line3_y + 8.0;
        let mut sx = 12.0;
        for [r, g, b] in &colors {
            draw_rectangle(sx, swatch_y, 18.0, 14.0, Color::from_rgba(*r, *g, *b, 255));
            sx += 22.0;
        }

        // ── Bottom-right: album art + sorting ──
        let mut art_bottom = 0.0_f32;
        if let Some(tex) = &self.album_art_texture {
            let art_size = 140.0;
            let ax = sw - art_size - 10.0;
            let ay = sh - art_size - 35.0;
            // Semi-transparent background behind art
            draw_rectangle(
                ax - 4.0,
                ay - 4.0,
                art_size + 8.0,
                art_size + 8.0,
                Color::from_rgba(0, 0, 0, 160),
            );
            draw_texture_ex(
                tex,
                ax,
                ay,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(Vec2::new(art_size, art_size)),
                    ..Default::default()
                },
            );
            art_bottom = ay + art_size + 4.0;
        }
        let axis_str = match self.primary_axis {
            Axis::X => "X",
            Axis::Y => "Y",
        };
        let pri_str = match self.sort_primary {
            Sort::Asc => "Asc",
            Sort::Desc => "Desc",
        };
        let sec_str = match self.sort_secondary {
            Sort::Asc => "Asc",
            Sort::Desc => "Desc",
        };
        let sort_text = format!(
            "Sort: Axis={} Primary={} Secondary={}",
            axis_str, pri_str, sec_str
        );
        let sm = sharp_measure(18.0, &sort_text);
        let sort_y = if art_bottom > 0.0 {
            art_bottom
        } else {
            sh - sm.height - 15.0
        };
        draw_rectangle(
            sw - sm.width - 19.0,
            sort_y,
            sm.width + 14.0,
            sm.height + 10.0,
            Color::from_rgba(0, 0, 0, 160),
        );
        sharp_text(
            &sort_text,
            sw - sm.width - 12.0,
            sort_y + sm.height + 3.0,
            18.0,
            Color::from_rgba(255, 255, 100, 255),
        );

        // ── Bottom-center: controls hint ──
        let hint = "? help | ESC quit | Space preview | -/+ speed | g/G gain | [/] window | F freq | 1-0 palette | E effect | N album art";
        let hm = sharp_measure(14.0, hint);
        let hx = (sw - hm.width) / 2.0;
        sharp_text(
            hint,
            hx,
            sh - 10.0,
            14.0,
            Color::from_rgba(100, 100, 100, 200),
        );
    }

    fn draw_help_overlay(&self) {
        let sw = screen_width();
        let sh = screen_height();

        draw_rectangle(0.0, 0.0, sw, sh, Color::from_rgba(0, 0, 0, 200));

        let x = sw / 2.0 - 280.0;
        let mut y = sh / 2.0 - 180.0;

        sharp_text("Keybinds", x, y, 28.0, WHITE);
        y += 40.0;

        let binds = [
            ("ESC / Q", "Quit"),
            ("?", "Toggle this help"),
            ("Space", "Toggle panel visualization preview"),
            ("-  /  +", "Decrease / increase transition speed"),
            ("g  /  G", "Decrease / increase gain"),
            ("[  /  ]", "Decrease / increase sample time window"),
            ("F", "Cycle frequency range preset"),
            ("1-9, 0", "Switch color palette"),
            ("E", "Cycle effect: Spectrum / Energy Wave / Pulse"),
            ("A", "Toggle primary axis (X / Y)"),
            ("P", "Toggle primary sort (Asc / Desc)"),
            ("S", "Toggle secondary sort (Asc / Desc)"),
            ("N", "Use album art colors from current track"),
            ("R", "Reset all panels to black"),
        ];

        for (key, desc) in &binds {
            sharp_text(key, x, y, 20.0, Color::from_rgba(255, 255, 100, 255));
            sharp_text(desc, x + 120.0, y, 20.0, WHITE);
            y += 26.0;
        }

        sharp_text(
            "(Gain only affects visuals, not your music volume)",
            x,
            y + 14.0,
            16.0,
            GRAY,
        );
    }

    // ── Settings changes ─────────────────────────────────────────────────

    fn change_palette(&mut self, index: usize) {
        if index < self.palette_names.len() {
            let palette_name = &self.palette_names[index];
            if let Some(colors) = crate::palettes::get_palette(palette_name) {
                self.current_palette_index = index;
                self.stop_album_art_watcher();
                let mut state = self.viz_state.lock();
                state.colors = colors.clone();
                state.track_title = None;
                let _ = self.visualizer_tx.send(VisualizerMsg::SetPalette(colors));
            }
        }
    }

    fn toggle_axis(&mut self) {
        self.primary_axis = match self.primary_axis {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        };
        self.send_sorting();
    }

    fn toggle_primary_sort(&mut self) {
        self.sort_primary = match self.sort_primary {
            Sort::Asc => Sort::Desc,
            Sort::Desc => Sort::Asc,
        };
        self.send_sorting();
    }

    fn toggle_secondary_sort(&mut self) {
        self.sort_secondary = match self.sort_secondary {
            Sort::Asc => Sort::Desc,
            Sort::Desc => Sort::Asc,
        };
        self.send_sorting();
    }

    fn send_sorting(&self) {
        let _ = self.visualizer_tx.send(VisualizerMsg::SetSorting {
            primary_axis: self.primary_axis,
            sort_primary: self.sort_primary,
            sort_secondary: self.sort_secondary,
            global_orientation: self.global_orientation,
        });
    }

    fn cycle_effect(&mut self) {
        self.effect = match self.effect {
            Effect::Spectrum => Effect::EnergyWave,
            Effect::EnergyWave => Effect::Pulse,
            Effect::Pulse => Effect::Spectrum,
        };
        let _ = self
            .visualizer_tx
            .send(VisualizerMsg::SetEffect(self.effect));
    }

    /// Fetches album art + palette on a background thread so the render loop
    /// never blocks on HTTP downloads or color extraction.
    fn use_album_art_palette(&mut self) {
        self.stop_album_art_watcher();
        let stop = Arc::new(AtomicBool::new(false));
        self.album_art_stop = Some(Arc::clone(&stop));
        let tx = self.visualizer_tx.clone();
        let viz_state = Arc::clone(&self.viz_state);
        std::thread::spawn(move || {
            // Initial fetch
            if let Some((artwork, colors)) = crate::now_playing::fetch_artwork_and_palette() {
                let title = crate::now_playing::get_track_title();
                let mut state = viz_state.lock();
                state.colors = colors.clone();
                state.track_title = title;
                state.artwork_bytes = Some(artwork);
                state.artwork_generation += 1;
                let _ = tx.send(VisualizerMsg::SetPalette(colors));
            }

            // Then poll for track changes
            let mut last_title = crate::now_playing::get_track_title();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let title = crate::now_playing::get_track_title();
                if title != last_title {
                    last_title = title.clone();
                    if let Some((artwork, colors)) = crate::now_playing::fetch_artwork_and_palette()
                    {
                        let mut state = viz_state.lock();
                        state.colors = colors.clone();
                        state.track_title = title;
                        state.artwork_bytes = Some(artwork);
                        state.artwork_generation += 1;
                        let _ = tx.send(VisualizerMsg::SetPalette(colors));
                    }
                }
            }
        });
    }

    fn stop_album_art_watcher(&mut self) {
        if let Some(stop) = self.album_art_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        let mut state = self.viz_state.lock();
        state.track_title = None;
        state.artwork_bytes = None;
    }
}

// ── Free-standing controller drawing ─────────────────────────────────────

fn draw_controller(
    x: f32,
    y: f32,
    _panel: &PanelInfo,
    scale: f32,
    all_panels: &[PanelInfo],
    transformed_positions: &[(f32, f32)],
) {
    // Find nearest parent light panel
    let mut min_dist = f32::MAX;
    let mut nearest_idx = 0;
    for (i, other) in all_panels.iter().enumerate() {
        if other.shape_type.side_length >= 1.0 {
            let (ox, oy) = transformed_positions[i];
            let dist = ((x - ox).powi(2) + (y - oy).powi(2)).sqrt();
            if dist < min_dist {
                min_dist = dist;
                nearest_idx = i;
            }
        }
    }

    let (parent_x, parent_y) = transformed_positions[nearest_idx];
    let parent = &all_panels[nearest_idx];
    let dx = x - parent_x;
    let dy = y - parent_y;
    let angle_to_ctrl = dy.atan2(dx);
    let num_sides = parent.shape_type.num_sides();
    let parent_side = parent.shape_type.side_length * scale;

    let parent_radius = match num_sides {
        3 => parent_side / f32::sqrt(3.0),
        4 => parent_side / f32::sqrt(2.0),
        _ => parent_side,
    };

    let parent_ori = (parent.orientation as f32).to_radians();
    let angle_per_side = 2.0 * PI / num_sides as f32;

    let mut closest_edge = 0;
    let mut min_angle_diff = f32::MAX;
    for i in 0..num_sides {
        let va = parent_ori + (i as f32 * angle_per_side);
        let raw = (angle_to_ctrl - va).abs() % (2.0 * PI);
        let diff = raw.min((2.0 * PI) - raw);
        if diff < min_angle_diff {
            min_angle_diff = diff;
            closest_edge = i;
        }
    }

    let v1a = parent_ori + (closest_edge as f32 * angle_per_side);
    let v2a = parent_ori + ((closest_edge + 1) as f32 * angle_per_side);
    let v1x = parent_x + parent_radius * v1a.cos();
    let v1y = parent_y + parent_radius * v1a.sin();
    let v2x = parent_x + parent_radius * v2a.cos();
    let v2y = parent_y + parent_radius * v2a.sin();

    let trap_h = 20.0;
    let mid_x = (v1x + v2x) / 2.0;
    let mid_y = (v1y + v2y) / 2.0;
    let pdx = mid_x - parent_x;
    let pdy = mid_y - parent_y;
    let plen = (pdx * pdx + pdy * pdy).sqrt();
    let pnx = pdx / plen;
    let pny = pdy / plen;
    let nr = 0.6;

    let verts = [
        Vec2::new(v1x, v1y),
        Vec2::new(v2x, v2y),
        Vec2::new(
            v2x + pnx * trap_h - (v2x - mid_x) * (1.0 - nr),
            v2y + pny * trap_h - (v2y - mid_y) * (1.0 - nr),
        ),
        Vec2::new(
            v1x + pnx * trap_h - (v1x - mid_x) * (1.0 - nr),
            v1y + pny * trap_h - (v1y - mid_y) * (1.0 - nr),
        ),
    ];

    let fill = Color::from_rgba(255, 200, 0, 255);
    draw_triangle(verts[0], verts[1], verts[2], fill);
    draw_triangle(verts[0], verts[2], verts[3], fill);

    let outline = Color::from_rgba(200, 150, 0, 255);
    for i in 0..4 {
        let next = (i + 1) % 4;
        draw_line(
            verts[i].x,
            verts[i].y,
            verts[next].x,
            verts[next].y,
            2.0,
            outline,
        );
    }

    let ts = 10.0;
    let td = sharp_measure(ts, "C");
    let lx = (verts[0].x + verts[1].x + verts[2].x + verts[3].x) / 4.0;
    let ly = (verts[0].y + verts[1].y + verts[2].y + verts[3].y) / 4.0;
    sharp_text("C", lx - td.width / 2.0, ly + ts / 3.0, ts, BLACK);
}

// ── Sharp text helpers (DPI-aware via camera_font_scale) ─────────────────

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn sharp_text(text: &str, x: f32, y: f32, logical_size: f32, color: Color) {
    let (fs, sx, sy) = camera_font_scale(logical_size);
    draw_text_ex(
        text,
        x,
        y,
        TextParams {
            font_size: fs,
            font_scale: sx,
            font_scale_aspect: sy / sx,
            color,
            ..Default::default()
        },
    );
}

fn sharp_measure(logical_size: f32, text: &str) -> TextDimensions {
    let (fs, sx, sy) = camera_font_scale(logical_size);
    measure_text(text, None, fs, sx * (sy / sx))
}

// ── Window icon ──────────────────────────────────────────────────────────

fn load_icon() -> miniquad::conf::Icon {
    fn decode_rgba(png_bytes: &[u8], size: u32) -> Vec<u8> {
        let img = image::load_from_memory(png_bytes)
            .expect("embedded icon PNG is valid")
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        img.into_raw()
    }

    let small = decode_rgba(include_bytes!("../Assets/icon_16.png"), 16);
    let medium = decode_rgba(include_bytes!("../Assets/icon_32.png"), 32);
    let big = decode_rgba(include_bytes!("../Assets/icon_64.png"), 64);

    miniquad::conf::Icon {
        small: small.try_into().expect("16x16 RGBA = 1024 bytes"),
        medium: medium.try_into().expect("32x32 RGBA = 4096 bytes"),
        big: big.try_into().expect("64x64 RGBA = 16384 bytes"),
    }
}
