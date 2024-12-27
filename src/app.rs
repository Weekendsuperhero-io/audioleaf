use crate::config::Config;
use crate::constants;
use crate::nanoleaf::NanoleafDevice;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Direction, Flex, Layout, Margin, Rect},
    prelude::Backend,
    style::{Style, Stylize},
    text::Line,
    widgets::{
        Block, Borders, Clear, HighlightSpacing, List, ListDirection, ListState, Paragraph,
        Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    Frame, Terminal,
};
use std::time::Duration;

#[derive(Debug)]
pub struct App {
    nl: NanoleafDevice,
    config: Config,
    list: Vec<String>,
    list_state: ListState,
    scroll: usize,
    scroll_state: ScrollbarState,
    show_help: bool,
    exit: bool,
}

impl App {
    pub fn new(nl: NanoleafDevice, config: Config) -> Result<Self, anyhow::Error> {
        let list = nl.get_effect_list()?;
        let list_state = ListState::default().with_selected(Some(0));

        Ok(App {
            nl,
            config,
            list,
            list_state,
            scroll: 0,
            scroll_state: ScrollbarState::default(),
            show_help: false,
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

        frame.render_stateful_widget(
            List::new(self.list.clone())
                .scroll_padding(2)
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .title_top(format!("{} Control Panel", self.nl.name)),
                )
                .highlight_style(Style::new().bold().cyan())
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
        frame.render_widget(
            Paragraph::new("Press '?' for help").block(Block::new().borders(Borders::ALL)),
            layout[1],
        );

        if self.show_help {
            let area = Self::popup_area(frame.area(), 90, 80);
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
        match key_event.code {
            KeyCode::Esc => {
                self.exit();
                Ok(())
            }
            KeyCode::Enter => self.play_effect(),
            KeyCode::Down => {
                self.scroll_by(1);
                Ok(())
            }
            KeyCode::Up => {
                self.scroll_by(-1);
                Ok(())
            }
            KeyCode::Char(c) => match c {
                // 'x' if key_event.modifiers.contains(KeyModifiers::ALT) => {
                //     panic!("you asked for it");
                // }
                'Q' => {
                    self.exit();
                    Ok(())
                }
                'V' => self.run_visualizer(),
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
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn run_visualizer(&self) -> Result<(), anyhow::Error> {
        self.nl.run_visualizer(self.config.port)
    }

    fn play_effect(&self) -> Result<(), anyhow::Error> {
        if let Some(selected) = self.list_state.selected() {
            self.nl.play_effect(&self.list[selected])
        } else {
            Ok(())
        }
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
