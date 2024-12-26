use crate::config::Config;
use crate::constants;
use crate::nanoleaf::NanoleafDevice;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Direction, Layout, Margin},
    style::{Style, Stylize},
    widgets::{
        Block, Borders, HighlightSpacing, List, ListDirection, ListState, Paragraph,
        ScrollDirection, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    DefaultTerminal, Frame,
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
            exit: false,
        })
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), anyhow::Error> {
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

        // self.list_state.select(Some(self.selected));
        frame.render_stateful_widget(
            List::new(self.list.clone())
                .scroll_padding(2)
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .title_top(format!("{} Control Panel", self.nl.name)),
                )
                .highlight_style(Style::new().italic())
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
            Paragraph::new("Controls: Q - quit, V - toggle visualizer, J/K or Up/Down - move through the effect list, Enter - choose effect").block(Block::new().borders(Borders::ALL)),
            layout[1],
        );
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
            KeyCode::Enter => self.play_effect(),
            KeyCode::Up => Ok(self.scroll_by(-1)),
            KeyCode::Down => Ok(self.scroll_by(1)),
            KeyCode::Char(c) => match c {
                'Q' => Ok(self.exit()),
                // vim-like scrolling
                'j' => Ok(self.scroll_by(1)),
                'd' if key_event.modifiers.contains(KeyModifiers::CONTROL) => Ok(self.scroll_by(3)),
                'k' => Ok(self.scroll_by(-1)),
                'u' if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    Ok(self.scroll_by(-3))
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    fn exit(&mut self) {
        self.exit = true;
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
}

// TODO:
// - async requests
// - why is the terminal freaking out after quitting with an error
// - pass json to utils::request_xxx instead of strings
