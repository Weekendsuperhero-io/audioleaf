use crate::audio;
use crate::constants;
use crate::event_handler::{self, Event};
use crate::utils;
use crate::visualizer::VisualizerMsg;
use crate::{
    config::{Axis, Sort, TuiConfig, VisualizerConfig},
    nanoleaf::{NlDevice, NlEffect},
    visualizer,
};
use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    layout::Margin,
    prelude::Backend,
    style::{Style, Stylize},
    text::Line,
    widgets::{
        Block, Borders, HighlightSpacing, List, ListDirection, ListItem, ListState, Paragraph,
        Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    Frame, Terminal,
};
use std::sync::mpsc;

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
    ToggleAxis,
    TogglePrimarySort,
    ToggleSecondarySort,
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

        let tx = visualizer::Visualizer::new(visualizer_config, audio_stream, &nl_device)?.init();

        // Initialize palette list
        let mut palette_names = crate::palettes::get_palette_names();
        palette_names.sort();

        let visualizer = Visualizer {
            tx,
            gain,
            current_palette_index: 0,
            palette_names,
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

    pub fn run(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
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
        Ok(())
    }

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
                KeyCode::Char('V') => match self.view {
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
                _ => AppMsg::NoOp,
            },
        }
    }

    fn update(&mut self, msg: AppMsg) -> Result<()> {
        match msg {
            AppMsg::NoOp => Ok(()),
            AppMsg::Quit => {
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
                    if let Some(hues) = crate::palettes::get_palette(palette_name) {
                        self.visualizer.current_palette_index = index;
                        self.visualizer.tx.send(VisualizerMsg::SetPalette(hues))?;
                    }
                }
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
                let current_palette = if self.visualizer.current_palette_index
                    < self.visualizer.palette_names.len()
                {
                    &self.visualizer.palette_names[self.visualizer.current_palette_index]
                } else {
                    "Unknown"
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

                let mut lines = vec![
                    Line::from("Music Visualizer".bold().cyan()),
                    Line::from(""),
                    Line::from(vec![
                        "Amplitude gain: ".into(),
                        format!("{:.2}", self.visualizer.gain).blue(),
                    ]),
                    Line::from(vec!["Current palette: ".into(), current_palette.green()]),
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
                ];

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
                        Line::from(vec!["V".bold(), " - toggle music visualizer mode".into()]),
                        Line::from(vec!["-/+".bold(), " - decrease/increase gain (in visualizer mode)".into()]),
                        Line::from(vec!["1-9, 0".bold(), " - switch color palette (in visualizer mode)".into()]),
                        Line::from(vec!["A".bold(), " - toggle primary axis X/Y (in visualizer mode)".into()]),
                        Line::from(vec!["P".bold(), " - toggle primary sort Asc/Desc (in visualizer mode)".into()]),
                        Line::from(vec!["S".bold(), " - toggle secondary sort Asc/Desc (in visualizer mode)".into()]),
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
