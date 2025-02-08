use crate::config::Config;
use crate::nanoleaf::NanoleafDevice;
use crate::utils;
use crate::visualizer::VisualizerEvent;
use crate::{constants, nanoleaf::Effect};
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Direction, Flex, Layout, Margin, Rect},
    prelude::Backend,
    style::{Style, Stylize},
    text::Line,
    widgets::{
        Block, Borders, Clear, HighlightSpacing, List, ListDirection, ListItem, ListState,
        Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    Frame, Terminal,
};
use rustfft::num_traits::Signed;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
enum AppMode {
    EffectsList,
    Visualizer,
}

#[derive(Debug)]
pub struct App {
    app_mode: AppMode,
    tx: mpsc::Sender<VisualizerEvent>,
    nl: NanoleafDevice,
    // config: Config,
    list: Vec<Effect>,
    list_state: ListState,
    scroll: usize,
    scroll_state: ScrollbarState,
    gain: f32,
    show_help: bool,
    use_colors: bool,
    exit: bool,
}

impl App {
    pub fn new(
        nl: NanoleafDevice,
        tx: mpsc::Sender<VisualizerEvent>,
        config: &Config,
    ) -> Result<Self, anyhow::Error> {
        let list = nl.get_effect_list()?;
        let list_pos = if let Some(ref curr_effect) = nl.curr_effect {
            list.iter()
                .position(|x| x.name == *curr_effect)
                .unwrap_or(0)
        } else {
            0
        };
        let list_state = ListState::default().with_selected(Some(list_pos));

        Ok(App {
            tx,
            app_mode: AppMode::EffectsList,
            nl,
            list,
            list_state,
            scroll: 0,
            scroll_state: ScrollbarState::default(),
            gain: config.visualizer_options.default_gain,
            show_help: false,
            use_colors: config.cli_options.use_colors,
            exit: false,
        })
    }

    pub fn run(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<(), anyhow::Error> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Percentage(90), Constraint::Percentage(10)])
            .split(frame.area());
        match self.app_mode {
            AppMode::EffectsList => {
                frame.render_stateful_widget(
                    List::new(self.list.iter().map(|x| {
                        let name = x.name.as_str();
                        if self.use_colors {
                            ListItem::new(utils::colorize_effect_name(name, &x.colors))
                        } else {
                            ListItem::new(name)
                        }
                    }))
                    .scroll_padding(2)
                    .block(
                        Block::new()
                            .borders(Borders::ALL)
                            .title_top(format!("{} Control Panel", self.nl.name)),
                    )
                    .highlight_style(Style::new().bold())
                    .highlight_symbol(">> ")
                    .highlight_spacing(HighlightSpacing::Always)
                    .direction(ListDirection::TopToBottom),
                    layout[0],
                    &mut self.list_state,
                );

                self.scroll_state = self.scroll_state.content_length(self.list.len());
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .track_symbol(Some("│"))
                        .begin_symbol(Some("↑"))
                        .end_symbol(Some("↓")),
                    layout[0].inner(Margin {
                        vertical: 1,
                        horizontal: 0,
                    }),
                    &mut self.scroll_state,
                );
            }
            AppMode::Visualizer => {
                frame.render_widget(
                    Paragraph::new(vec![
                        Line::raw("Visualizer mode ON"),
                        Line::raw(format!("Amplitude gain: {:.2}", self.gain)),
                    ])
                    .block(
                        Block::new()
                            .borders(Borders::ALL)
                            .title_top(format!("{} Control Panel", self.nl.name)),
                    )
                    .centered(),
                    layout[0],
                );
            }
        }
        frame.render_widget(
            Paragraph::new("Press '?' for help").block(Block::new().borders(Borders::ALL)),
            layout[1],
        );

        if self.show_help {
            let area = Self::popup_area(frame.area(), 90, 75);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::raw("Controls:").bold(),
                    Line::raw("* ? - toggle help"),
                    Line::raw("* Q or Esc - quit"),
                    Line::raw("* V - toggle music visualizer mode"),
                    Line::raw("* Down/Up or j/k - scroll down/up"),
                    Line::raw("* C-d/C-u - scroll down/up by 3 items"),
                    Line::raw("* g/G - go to the top/bottom of the list"),
                    Line::raw("* -/+ - decrease/increase gain (in visualizer mode)"),
                    Line::raw("* (note that this doesn't affect your music volume, only the visuals are amplified)"),
                    Line::raw("* Enter - play selected effect"),
                ])
                .block(Block::new().borders(Borders::ALL).title("Help")),
                area,
            );
        }
    }

    fn popup_area(area: Rect, pc_x: u16, pc_y: u16) -> Rect {
        let hori = Layout::horizontal([Constraint::Percentage(pc_x)]).flex(Flex::Center);
        let vert = Layout::vertical([Constraint::Percentage(pc_y)]).flex(Flex::Center);
        let [area] = vert.areas(area);
        let [area] = hori.areas(area);
        area
    }

    fn handle_events(&mut self) -> Result<(), anyhow::Error> {
        if event::poll(Duration::from_millis(constants::TICKRATE))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event)?
                }
                _ => {}
            };
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<(), anyhow::Error> {
        match self.app_mode {
            AppMode::EffectsList => match key_event.code {
                KeyCode::Esc => self.exit(),
                KeyCode::Enter => {
                    if let Some(selected) = self.list_state.selected() {
                        let effect = self.list[selected].clone();
                        self.play_effect(&effect.name)?;
                    }
                    Ok(())
                }
                KeyCode::Down => {
                    self.scroll_by(1);
                    Ok(())
                }
                KeyCode::Up => {
                    self.scroll_by(-1);
                    Ok(())
                }
                KeyCode::Char(c) => match c {
                    'Q' => self.exit(),
                    'V' => self.toggle_visualizer(),
                    // vim-like scrolling
                    'j' => {
                        self.scroll_by(1);
                        Ok(())
                    }
                    'k' => {
                        self.scroll_by(-1);
                        Ok(())
                    }
                    'd' if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.scroll_by(3);
                        Ok(())
                    }
                    'u' if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.scroll_by(-3);
                        Ok(())
                    }
                    'g' => {
                        self.scroll_to_start();
                        Ok(())
                    }
                    'G' => {
                        self.scroll_to_end();
                        Ok(())
                    }
                    '?' => {
                        self.toggle_help();
                        Ok(())
                    }
                    _ => Ok(()),
                },
                _ => Ok(()),
            },
            AppMode::Visualizer => match key_event.code {
                KeyCode::Esc => self.exit(),
                KeyCode::Char(c) => match c {
                    'Q' => self.exit(),
                    'V' => self.toggle_visualizer(),
                    '-' | '_' => self.change_gain(-0.05),
                    '+' | '=' => self.change_gain(0.05),
                    '?' => {
                        self.toggle_help();
                        Ok(())
                    }
                    _ => Ok(()),
                },
                _ => Ok(()),
            },
        }
    }

    fn exit(&mut self) -> Result<(), anyhow::Error> {
        self.tx.send(VisualizerEvent::End)?;
        self.exit = true;
        Ok(())
    }

    fn toggle_visualizer(&mut self) -> Result<(), anyhow::Error> {
        match self.app_mode {
            AppMode::EffectsList => {
                self.nl.request_external_control()?;
                self.tx.send(VisualizerEvent::Resume)?;
                self.app_mode = AppMode::Visualizer;
            }
            AppMode::Visualizer => {
                self.tx.send(VisualizerEvent::Pause)?;
                if let Some(effect) = self.nl.curr_effect.clone() {
                    Self::play_effect(self, &effect)?;
                }
                self.app_mode = AppMode::EffectsList;
            }
        }
        Ok(())
    }

    fn play_effect(&mut self, effect: &str) -> Result<(), anyhow::Error> {
        self.nl.play_effect(effect)?;
        self.nl.curr_effect = Some(effect.to_string());
        Ok(())
    }

    fn change_gain(&mut self, delta: f32) -> Result<(), anyhow::Error> {
        if (self.gain + delta).is_positive() {
            self.tx.send(VisualizerEvent::GainDelta(delta))?;
            self.gain += delta;
        }
        Ok(())
    }

    fn scroll_by(&mut self, k: i16) {
        if k < 0 {
            self.list_state.scroll_up_by(k.unsigned_abs());
            self.scroll = self.scroll.saturating_sub(k.unsigned_abs() as usize);
        } else {
            self.list_state.scroll_down_by(k as u16);
            self.scroll = self.scroll.saturating_add(k as usize);
        }
        self.scroll_state = self.scroll_state.position(self.scroll);
    }

    fn scroll_to_start(&mut self) {
        self.list_state.select_first();
        self.scroll = 0;
        self.scroll_state = self.scroll_state.position(self.scroll);
    }

    fn scroll_to_end(&mut self) {
        self.list_state.select_last();
        self.scroll = self.list.len() - 1;
        self.scroll_state = self.scroll_state.position(self.scroll);
    }

    fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
}
