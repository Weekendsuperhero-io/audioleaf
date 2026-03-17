use crate::audio;
use crate::constants;
use crate::event_handler::{self, Event};
use crate::utils;
use crate::visualizer::VisualizerMsg;
use crate::{
    config::{Axis, Effect, Sort, TuiConfig, VisualizerConfig},
    nanoleaf::{NlDevice, NlEffect},
    visualizer,
};
use anyhow::Result;
use ratatui::{
    Frame, Terminal,
    crossterm::event::KeyCode,
    layout::Margin,
    prelude::Backend,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, HighlightSpacing, List, ListDirection, ListItem, ListState, Paragraph,
        Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

#[derive(Debug, Default)]
enum AppState {
    #[default]
    RunningEffectList,
    RunningVisualizer,
    Done,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AppView {
    #[default]
    EffectList,
    Visualizer,
    HelpScreen,
}

#[derive(Debug)]
enum AppMsg {
    NoOp,
    Quit,
    ChangeView(AppView),
    PlayEffect(usize),
    ScrollDown(u16),
    ScrollUp(u16),
    ScrollToBottom,
    ScrollToTop,
    ChangeGain(f32),
    ChangePalette(usize),
    CycleEffect,
    ResetPanels,
    UseAlbumArtPalette,
    ToggleAxis,
    TogglePrimarySort,
    ToggleSecondarySort,
}

/// Display state shared between the main thread and the album art watcher thread.
#[derive(Debug)]
struct VizState {
    /// The RGB colors currently driving the visualizer palette.
    colors: Vec<[u8; 3]>,
    /// Set when album art mode is active; cleared when switching to a named palette.
    track_title: Option<String>,
}

#[derive(Debug)]
struct Scroll {
    pos: u16,
    state: ScrollbarState,
}

#[derive(Debug)]
struct EffectList {
    list: Vec<NlEffect>,
    state: ListState,
    scroll: Scroll,
}

#[derive(Debug)]
struct Visualizer {
    tx: mpsc::Sender<VisualizerMsg>,
    gain: f32,
    current_palette_index: usize,
    palette_names: Vec<String>,
    viz_state: Arc<Mutex<VizState>>,
    album_art_stop: Option<Arc<AtomicBool>>,
    effect: Effect,
    primary_axis: Axis,
    sort_primary: Sort,
    sort_secondary: Sort,
    global_orientation: u16,
}

#[derive(Debug)]
pub struct App {
    state: AppState,
    prev_view: AppView,
    view: AppView,
    nl_device: NlDevice,
    effect_list: EffectList,
    visualizer: Visualizer,
    display_colors: bool,
}

impl App {
    /// Constructs a new `App` for TUI-based Nanoleaf effect selection and audio visualizer.
    ///
    /// Initializes:
    /// - Effect list from device API, selects current effect if running.
    /// - Visualizer with audio stream, UDP, config params, starts processing thread via `init()`.
    /// - State to EffectList view, running effects mode.
    /// - Palette names from predefined for switching.
    /// - Colorful names from TUI config.
    /// - Fetches device global orientation for panel sorting.
    ///
    /// # Arguments
    ///
    /// * `nl_device` - Connected Nanoleaf device.
    /// * `tui_config` - UI settings like colorful effect names.
    /// * `visualizer_config` - Audio/viz params like gain, hues, sorting.
    ///
    /// # Errors
    ///
    /// From device API, visualizer new/init, or effect list fetch.
    pub fn new(
        nl_device: NlDevice,
        tui_config: TuiConfig,
        visualizer_config: VisualizerConfig,
    ) -> Result<Self> {
        let state = AppState::default();
        let prev_view = AppView::default();
        let view = AppView::default();
        let list = nl_device.get_effect_list()?;
        let list_pos = if let Some(cur_effect_name) = nl_device.cur_effect_name.as_deref() {
            list.iter()
                .position(|effect| effect.name == cur_effect_name)
                .unwrap_or(0)
        } else {
            0
        };
        let scroll = Scroll {
            pos: 0,
            state: ScrollbarState::default(),
        };
        let effect_list = EffectList {
            list,
            state: ListState::default().with_selected(Some(list_pos)),
            scroll,
        };
        let audio_stream = audio::AudioStream::new(visualizer_config.audio_backend.as_deref())?;
        let gain = visualizer_config
            .default_gain
            .unwrap_or(constants::DEFAULT_GAIN);
        eprintln!("INFO: Starting with gain: {}", gain);

        // Get global orientation
        let global_orientation = nl_device
            .get_global_orientation()
            .ok()
            .and_then(|o| o["value"].as_u64())
            .unwrap_or(0) as u16;

        let primary_axis = visualizer_config.primary_axis.unwrap_or_default();
        let sort_primary = visualizer_config.sort_primary.unwrap_or_default();
        let sort_secondary = visualizer_config.sort_secondary.unwrap_or_default();
        let effect = visualizer_config.effect.unwrap_or_default();

        let initial_colors = visualizer_config
            .colors
            .clone()
            .unwrap_or_else(|| Vec::from(constants::DEFAULT_COLORS));

        let tx = visualizer::Visualizer::new(visualizer_config, audio_stream, &nl_device)?.init();

        // Initialize palette list
        let mut palette_names = crate::palettes::get_palette_names();
        palette_names.sort();
        let viz_state = Arc::new(Mutex::new(VizState {
            colors: initial_colors,
            track_title: None,
        }));

        let visualizer = Visualizer {
            tx,
            gain,
            current_palette_index: 0,
            palette_names,
            viz_state,
            album_art_stop: None,
            effect,
            primary_axis,
            sort_primary,
            sort_secondary,
            global_orientation,
        };
        let display_colors = tui_config
            .colorful_effect_names
            .unwrap_or(constants::DEFAULT_COLORFUL_EFFECT_NAMES);

        Ok(App {
            state,
            prev_view,
            view,
            nl_device,
            effect_list,
            visualizer,
            display_colors,
        })
    }

    /// Executes the main TUI application loop.
    ///
    /// Creates event handler for key/tick events.
    /// Loop: draw current view, receive event, map to AppMsg, update app state.
    /// Breaks when state=Done (quit).
    /// On exit, sends End msg to visualizer to stop audio/UDP thread.
    ///
    /// # Arguments
    ///
    /// * `self` - Mutable app state.
    /// * `terminal` - Ratatui terminal for rendering frames.
    ///
    /// # Errors
    ///
    /// From event recv, update logic, or draw.
    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B::Error: Send + Sync + 'static,
    {
        let event_handler = event_handler::EventHandler::new();
        loop {
            terminal.draw(|frame| self.render_view(frame))?;
            let event = event_handler.next()?;
            let msg = self.event_to_msg(event);
            self.update(msg)?;
            if let AppState::Done = self.state {
                break;
            }
        }
        self.visualizer.tx.send(VisualizerMsg::End)?;
        self.shutdown()
    }

    /// Shuts down the device by turning it off and setting brightness to 0.
    ///
    /// Called after the main TUI loop exits to restore the device to an off state.
    pub fn shutdown(&self) -> Result<()> {
        self.nl_device.set_state(Some(false), Some(0))?;
        Ok(())
    }

    /// Converts raw terminal events to `AppMsg` for state updates.
    ///
    /// Ignores ticks (NoOp).
    /// Maps KeyEvent codes to actions:
    /// - ESC/Q: Quit
    /// - Enter: Play selected effect in list view
    /// - Up/Down/j/k: Scroll list
    /// - g/G: Scroll top/bottom
    /// - V: Toggle EffectList <-> Visualizer view
    /// - ?: Toggle help screen
    /// - +/-=_ : Adjust visualizer gain by ±0.05
    /// - 0-9: Switch to numbered palette
    /// - a/A: Toggle primary axis X/Y
    /// - p/P: Toggle primary sort Asc/Desc
    /// - s/S: Toggle secondary sort Asc/Desc
    /// - e/E: Cycle visual effect (Spectrum / Energy Wave)
    /// - Defaults to NoOp for unhandled.
    ///
    /// View-specific logic, e.g., scroll only in EffectList.
    fn event_to_msg(&self, event: Event) -> AppMsg {
        match event {
            Event::Tick => AppMsg::NoOp,
            Event::Key(e) => match e.code {
                KeyCode::Esc | KeyCode::Char('Q') => AppMsg::Quit,
                KeyCode::Enter => {
                    if let AppView::EffectList = self.view {
                        if let Some(selected) = self.effect_list.state.selected() {
                            AppMsg::PlayEffect(selected)
                        } else {
                            AppMsg::NoOp
                        }
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let AppView::EffectList = self.view {
                        AppMsg::ScrollDown(1)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let AppView::EffectList = self.view {
                        AppMsg::ScrollUp(1)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('g') => AppMsg::ScrollToTop,
                KeyCode::Char('G') => AppMsg::ScrollToBottom,
                KeyCode::Char('V') | KeyCode::Char('v') => match self.view {
                    AppView::HelpScreen => AppMsg::NoOp,
                    AppView::EffectList => AppMsg::ChangeView(AppView::Visualizer),
                    AppView::Visualizer => AppMsg::ChangeView(AppView::EffectList),
                },
                KeyCode::Char('?') => {
                    if let AppView::HelpScreen = self.view {
                        AppMsg::ChangeView(self.prev_view)
                    } else {
                        AppMsg::ChangeView(AppView::HelpScreen)
                    }
                }
                KeyCode::Char('-') | KeyCode::Char('_') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangeGain(-0.05)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('=') | KeyCode::Char('+') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangeGain(0.05)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('1') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(0)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('2') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(1)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('3') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(2)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('4') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(3)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('5') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(4)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('6') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(5)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('7') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(6)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('8') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(7)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('9') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(8)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('0') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ChangePalette(9)
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ToggleAxis
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::TogglePrimarySort
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ToggleSecondarySort
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::CycleEffect
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::UseAlbumArtPalette
                    } else {
                        AppMsg::NoOp
                    }
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    if let AppView::Visualizer = self.view {
                        AppMsg::ResetPanels
                    } else {
                        AppMsg::NoOp
                    }
                }
                _ => AppMsg::NoOp,
            },
        }
    }

    /// Signals the album art watcher thread to stop, if one is running.
    fn stop_album_art_watcher(&mut self) {
        if let Some(stop) = self.visualizer.album_art_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Ok(mut state) = self.visualizer.viz_state.lock() {
            state.track_title = None;
        }
    }

    /// Spawns a background thread that polls the current track title every 3 seconds.
    /// When the title changes it fetches the new album art and sends SetPalette directly
    /// to the visualizer thread. Stopped by setting the AtomicBool stop flag.
    fn start_album_art_watcher(&mut self) {
        self.stop_album_art_watcher();
        let stop = Arc::new(AtomicBool::new(false));
        self.visualizer.album_art_stop = Some(Arc::clone(&stop));
        let tx = self.visualizer.tx.clone();
        let viz_state = Arc::clone(&self.visualizer.viz_state);
        std::thread::spawn(move || {
            let mut last_title = crate::now_playing::get_track_title();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let title = crate::now_playing::get_track_title();
                if title != last_title {
                    last_title = title.clone();
                    if let Some(colors) = crate::now_playing::fetch_palette() {
                        if let Ok(mut state) = viz_state.lock() {
                            state.colors = colors.clone();
                            state.track_title = title;
                        }
                        let _ = tx.send(VisualizerMsg::SetPalette(colors));
                    }
                }
            }
        });
    }

    /// Applies an `AppMsg` to update application state, views, or external components.
    ///
    /// Match on msg type:
    /// - NoOp/Quit: Idle or set Done state.
    /// - Scroll: Adjust effect list selection and scrollbar position.
    /// - View change: Switch views, pause/resume visualizer or effects mode, enable UDP if needed.
    /// - PlayEffect: Calls device.play_effect by selected name.
    /// - ChangeGain/Palette: Send SetGain/SetPalette to visualizer tx.
    /// - Toggles (Axis/Sorts): Flip enum, send SetSorting with current params to visualizer.
    ///
    /// Syncs list state with scrollbar for rendering.
    /// Ensures state transitions (e.g., resume viz only if switching to it).
    ///
    /// # Errors
    ///
    /// From device API calls or visualizer msg send.
    fn update(&mut self, msg: AppMsg) -> Result<()> {
        match msg {
            AppMsg::NoOp => Ok(()),
            AppMsg::Quit => {
                self.stop_album_art_watcher();
                self.state = AppState::Done;
                Ok(())
            }
            AppMsg::ScrollDown(k) => {
                self.effect_list.state.scroll_down_by(k);
                self.effect_list.scroll.pos = self.effect_list.scroll.pos.saturating_add(k);
                self.effect_list.scroll.state = self
                    .effect_list
                    .scroll
                    .state
                    .position(self.effect_list.scroll.pos as usize);
                Ok(())
            }
            AppMsg::ScrollUp(k) => {
                self.effect_list.state.scroll_up_by(k);
                self.effect_list.scroll.pos = self.effect_list.scroll.pos.saturating_sub(k);
                self.effect_list.scroll.state = self
                    .effect_list
                    .scroll
                    .state
                    .position(self.effect_list.scroll.pos as usize);
                Ok(())
            }
            AppMsg::ScrollToBottom => {
                self.effect_list.state.select_last();
                self.effect_list.scroll.pos = self.effect_list.list.len() as u16 - 1;
                self.effect_list.scroll.state = self
                    .effect_list
                    .scroll
                    .state
                    .position(self.effect_list.scroll.pos as usize);
                Ok(())
            }
            AppMsg::ScrollToTop => {
                self.effect_list.state.select_first();
                self.effect_list.scroll.pos = 0;
                self.effect_list.scroll.state = self
                    .effect_list
                    .scroll
                    .state
                    .position(self.effect_list.scroll.pos as usize);
                Ok(())
            }
            AppMsg::ChangeView(view) => {
                self.prev_view = self.view;
                self.view = view;
                match self.view {
                    AppView::Visualizer => {
                        if !matches!(self.state, AppState::RunningVisualizer) {
                            self.nl_device.request_udp_control()?;
                            self.visualizer.tx.send(VisualizerMsg::Resume)?;
                        }
                        self.state = AppState::RunningVisualizer;
                    }
                    AppView::EffectList => {
                        if !matches!(self.state, AppState::RunningEffectList) {
                            self.visualizer.tx.send(VisualizerMsg::Pause)?;
                        }
                        self.state = AppState::RunningEffectList;
                    }
                    AppView::HelpScreen => (),
                };
                Ok(())
            }
            AppMsg::PlayEffect(i) => {
                let effect_name = self.effect_list.list[i].name.as_str();
                self.nl_device.play_effect(effect_name)?;
                Ok(())
            }
            AppMsg::ChangeGain(delta) => {
                self.visualizer.gain += delta;
                self.visualizer
                    .tx
                    .send(VisualizerMsg::SetGain(self.visualizer.gain))?;
                Ok(())
            }
            AppMsg::ChangePalette(index) => {
                if index < self.visualizer.palette_names.len() {
                    let palette_name = &self.visualizer.palette_names[index];
                    if let Some(colors) = crate::palettes::get_palette(palette_name) {
                        self.visualizer.current_palette_index = index;
                        self.stop_album_art_watcher();
                        if let Ok(mut state) = self.visualizer.viz_state.lock() {
                            state.colors = colors.clone();
                            state.track_title = None;
                        }
                        self.visualizer.tx.send(VisualizerMsg::SetPalette(colors))?;
                    }
                }
                Ok(())
            }
            AppMsg::UseAlbumArtPalette => {
                eprintln!("DEBUG app: UseAlbumArtPalette triggered");
                if let Some(colors) = crate::now_playing::fetch_palette() {
                    let title = crate::now_playing::get_track_title();
                    if let Ok(mut state) = self.visualizer.viz_state.lock() {
                        state.colors = colors.clone();
                        state.track_title = title;
                    }
                    self.visualizer.tx.send(VisualizerMsg::SetPalette(colors))?;
                    self.start_album_art_watcher();
                }
                Ok(())
            }
            AppMsg::CycleEffect => {
                self.visualizer.effect = match self.visualizer.effect {
                    Effect::Spectrum => Effect::EnergyWave,
                    Effect::EnergyWave => Effect::Pulse,
                    Effect::Pulse => Effect::Spectrum,
                };
                self.visualizer
                    .tx
                    .send(VisualizerMsg::SetEffect(self.visualizer.effect))?;
                Ok(())
            }
            AppMsg::ResetPanels => {
                self.visualizer.tx.send(VisualizerMsg::ResetPanels)?;
                Ok(())
            }
            AppMsg::ToggleAxis => {
                self.visualizer.primary_axis = match self.visualizer.primary_axis {
                    Axis::X => Axis::Y,
                    Axis::Y => Axis::X,
                };
                self.visualizer.tx.send(VisualizerMsg::SetSorting {
                    primary_axis: self.visualizer.primary_axis,
                    sort_primary: self.visualizer.sort_primary,
                    sort_secondary: self.visualizer.sort_secondary,
                    global_orientation: self.visualizer.global_orientation,
                })?;
                Ok(())
            }
            AppMsg::TogglePrimarySort => {
                self.visualizer.sort_primary = match self.visualizer.sort_primary {
                    Sort::Asc => Sort::Desc,
                    Sort::Desc => Sort::Asc,
                };
                self.visualizer.tx.send(VisualizerMsg::SetSorting {
                    primary_axis: self.visualizer.primary_axis,
                    sort_primary: self.visualizer.sort_primary,
                    sort_secondary: self.visualizer.sort_secondary,
                    global_orientation: self.visualizer.global_orientation,
                })?;
                Ok(())
            }
            AppMsg::ToggleSecondarySort => {
                self.visualizer.sort_secondary = match self.visualizer.sort_secondary {
                    Sort::Asc => Sort::Desc,
                    Sort::Desc => Sort::Asc,
                };
                self.visualizer.tx.send(VisualizerMsg::SetSorting {
                    primary_axis: self.visualizer.primary_axis,
                    sort_primary: self.visualizer.sort_primary,
                    sort_secondary: self.visualizer.sort_secondary,
                    global_orientation: self.visualizer.global_orientation,
                })?;
                Ok(())
            }
        }
    }

    /// Renders the active view (effect list, visualizer, or help) into the terminal frame.
    ///
    /// Creates bordered main block with device name in magenta left title, right-aligned "? for help".
    /// Dispatches to view-specific rendering:
    /// - `EffectList`: Stateful list of NlEffect items, optional colorful char styling via palette,
    ///   highlight with >> symbol, synced scrollbar with padding.
    /// - Other views (Visualizer/HelpScreen): Implementation details for spectrum display or key bindings.
    /// - Updates scrollbar state post-render if needed.
    fn render_view(&mut self, frame: &mut Frame) {
        let main_block = Block::new()
            .borders(Borders::ALL)
            .title_top(
                Line::from(vec![
                    "Connected to ".into(),
                    self.nl_device.name.as_str().magenta(),
                ])
                .left_aligned(),
            )
            .title_top(
                Line::from(vec!["Press ".into(), "?".magenta(), " for help".into()])
                    .right_aligned(),
            );
        match self.view {
            AppView::EffectList => {
                frame.render_stateful_widget(
                    List::new(self.effect_list.list.iter().map(|x| {
                        let name = x.name.as_str();
                        if self.display_colors {
                            ListItem::new(utils::colorful_effect_name(name, &x.palette))
                        } else {
                            ListItem::new(name)
                        }
                    }))
                    .scroll_padding(2)
                    .block(main_block)
                    .highlight_style(Style::new().bold())
                    .highlight_symbol(">> ")
                    .highlight_spacing(HighlightSpacing::Always)
                    .direction(ListDirection::TopToBottom),
                    frame.area(),
                    &mut self.effect_list.state,
                );
                self.effect_list.scroll.state = self
                    .effect_list
                    .scroll
                    .state
                    .content_length(self.effect_list.list.len());
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .track_symbol(Some("│"))
                        .begin_symbol(Some("↑"))
                        .end_symbol(Some("↓")),
                    frame.area().inner(Margin {
                        vertical: 1,
                        horizontal: 0,
                    }),
                    &mut self.effect_list.scroll.state,
                );
            }
            AppView::Visualizer => {
                // Snapshot display state from shared VizState (also written by watcher thread).
                let (current_palette_name, track_title, palette_colors) = {
                    let state = self.visualizer.viz_state.lock().unwrap();
                    let name = if state.track_title.is_some() {
                        "album-art".to_string()
                    } else if self.visualizer.current_palette_index
                        < self.visualizer.palette_names.len()
                    {
                        self.visualizer.palette_names[self.visualizer.current_palette_index]
                            .clone()
                    } else {
                        "Unknown".to_string()
                    };
                    (name, state.track_title.clone(), state.colors.clone())
                };
                let current_palette = current_palette_name.as_str();

                let effect_str = match self.visualizer.effect {
                    Effect::Spectrum => "Spectrum",
                    Effect::EnergyWave => "Energy Wave",
                    Effect::Pulse => "Pulse",
                };

                let axis_str = match self.visualizer.primary_axis {
                    Axis::X => "X",
                    Axis::Y => "Y",
                };
                let primary_str = match self.visualizer.sort_primary {
                    Sort::Asc => "Asc",
                    Sort::Desc => "Desc",
                };
                let secondary_str = match self.visualizer.sort_secondary {
                    Sort::Asc => "Asc",
                    Sort::Desc => "Desc",
                };

                // Build color swatch line: two spaces per color with background set.
                let mut swatch_spans: Vec<Span> = vec!["Colors: ".into()];
                for [r, g, b] in &palette_colors {
                    swatch_spans.push(Span::styled(
                        "  ",
                        Style::default().bg(Color::Rgb(*r, *g, *b)),
                    ));
                    swatch_spans.push(" ".into());
                }

                let mut lines = vec![
                    Line::from("Music Visualizer".bold().cyan()),
                    Line::from(""),
                    Line::from(vec![
                        "Amplitude gain: ".into(),
                        format!("{:.2}", self.visualizer.gain).blue(),
                    ]),
                    Line::from(vec!["Current palette: ".into(), current_palette.green()]),
                ];
                if let Some(title) = &track_title {
                    lines.push(Line::from(vec![
                        "Now playing: ".into(),
                        title.as_str().cyan().italic(),
                    ]));
                }
                lines.push(Line::from(swatch_spans));
                lines.extend([
                    Line::from(vec!["Effect [E]: ".into(), effect_str.magenta()]),
                    Line::from(""),
                    Line::from("Panel Sorting:".bold()),
                    Line::from(vec![
                        "  Primary Axis [A]: ".into(),
                        axis_str.yellow(),
                        "  |  Primary [P]: ".into(),
                        primary_str.yellow(),
                        "  |  Secondary [S]: ".into(),
                        secondary_str.yellow(),
                    ]),
                    Line::from(""),
                    Line::from("Available Palettes (press number to switch):".bold()),
                ]);

                for (i, palette_name) in self.visualizer.palette_names.iter().enumerate().take(10) {
                    let key = if i == 9 {
                        "0".to_string()
                    } else {
                        (i + 1).to_string()
                    };
                    let is_current = i == self.visualizer.current_palette_index;
                    let line = if is_current {
                        Line::from(vec![
                            key.bold().yellow(),
                            " - ".into(),
                            palette_name.as_str().green().bold(),
                            " ◀".green(),
                        ])
                    } else {
                        Line::from(vec![key.bold(), " - ".into(), palette_name.as_str().into()])
                    };
                    lines.push(line);
                }

                frame.render_widget(
                    Paragraph::new(lines).block(main_block).centered(),
                    frame.area(),
                );
            }
            AppView::HelpScreen => {
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::from("Keybinds:".bold()),
                        Line::from(vec!["?".bold(), " - toggle help".into()]),
                        Line::from(vec!["Q/Esc".bold(), " - quit".into()]),
                        Line::from(vec!["g/G".bold(), " - go to the top/bottom of the list".into()]),
                        Line::from(vec!["j/Down, k/Up".bold(), " - scroll down and up".into()]),
                        Line::from(vec!["Enter".bold(), " - play selected effect".into()]),
                        Line::from(vec!["V/v".bold(), " - toggle music visualizer mode".into()]),
                        Line::from(vec!["-/+".bold(), " - decrease/increase gain (in visualizer mode)".into()]),
                        Line::from(vec!["1-9, 0".bold(), " - switch color palette (in visualizer mode)".into()]),
                        Line::from(vec!["A".bold(), " - toggle primary axis X/Y (in visualizer mode)".into()]),
                        Line::from(vec!["P".bold(), " - toggle primary sort Asc/Desc (in visualizer mode)".into()]),
                        Line::from(vec!["S".bold(), " - toggle secondary sort Asc/Desc (in visualizer mode)".into()]),
                        Line::from(vec!["E".bold(), " - cycle visual effect: Spectrum / Energy Wave / Pulse (in visualizer mode)".into()]),
                        Line::from(vec!["N".bold(), " - use album art colors from current track (in visualizer mode)".into()]),
                        Line::from(vec!["R".bold(), " - reset all panels to black (in visualizer mode)".into()]),
                        Line::from(vec!["(note that gain doesn't affect your music volume, only the visuals are amplified)".italic()]),
                    ])
                    .block(main_block)
                    .centered(),
                    frame.area(),
                );
            }
        };
    }
}
