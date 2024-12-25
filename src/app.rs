use crate::config::Config;
use crate::nanoleaf::NanoleafDevice;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Constraint, Direction, Layout, Margin},
    style::{Style, Stylize},
    widgets::{
        Block, Borders, List, ListDirection, ListState, Paragraph, ScrollDirection, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
    DefaultTerminal, Frame,
};

#[derive(Debug)]
pub struct App {
    nl: NanoleafDevice,
    config: Config,
    list: Vec<String>,
    list_state: ListState,
    scrollbar_state: ScrollbarState,
    exit: bool,
}

impl App {
    pub fn new(nl: NanoleafDevice, config: Config) -> Result<Self, anyhow::Error> {
        let list = nl.get_effect_list()?;
        let list_state = ListState::default();
        let scrollbar_state = ScrollbarState::default();

        Ok(App {
            nl,
            config,
            list,
            list_state,
            scrollbar_state,
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
            .constraints(vec![
                Constraint::Percentage(10),
                Constraint::Percentage(80),
                Constraint::Percentage(10),
            ])
            .split(frame.area());
        frame.render_widget(
            Paragraph::new(format!("Control Panel: {}", self.nl.name))
                .block(Block::new().borders(Borders::ALL)),
            layout[0],
        );
        frame.render_stateful_widget(
            List::new(self.list.clone())
                .scroll_padding(2)
                .block(Block::new().borders(Borders::BOTTOM))
                .highlight_style(Style::new().italic())
                .highlight_symbol(">> ")
                .direction(ListDirection::TopToBottom),
            layout[1],
            &mut self.list_state,
        );
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("v"))
                .end_symbol(Some("^")),
            layout[1].inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut self.scrollbar_state,
        );
        frame.render_widget(
            Paragraph::new("Controls: Q - quit, V - toggle visualizer, J/K or Up/Down - move through the effect list, Enter - choose effect").block(Block::new().borders(Borders::ALL)),
            layout[2],
        );
    }

    fn handle_events(&mut self) -> Result<(), anyhow::Error> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::Char(x) => {
                if x == 'Q' {
                    self.exit();
                } else {
                    // self.add_to_list(format!("typed char: {}", x));
                }
            }
            KeyCode::Tab => self.add_to_list("tab pressed".to_string()),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn scroll_by(&mut self, k: i16) {
        if k < 0 {
            self.list_state.scroll_up_by(k.unsigned_abs());
            self.scrollbar_state.scroll(ScrollDirection::Backward);
        } else {
            self.list_state.scroll_down_by(k as u16);
            self.scrollbar_state.scroll(ScrollDirection::Forward);
        }
    }

    fn add_to_list(&mut self, s: String) {
        self.list.push(s);
        self.scrollbar_state = ScrollbarState::new(self.list.len());
    }
}
