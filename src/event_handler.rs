use crate::{constants, panic};
use anyhow::Result;
use ratatui::crossterm::event;
use std::{sync::mpsc, thread, time::Duration};

pub enum Event {
    Key(event::KeyEvent),
    Tick,
}

pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
}

impl EventHandler {
    pub fn new() -> Self {
        let tickrate = Duration::from_millis(constants::TICKRATE);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            panic::register_backtrace_panic_handler();
            loop {
                if event::poll(tickrate).expect("event poll failed") {
                    let event = event::read().expect("event read failed");
                    if let event::Event::Key(key_event) = event {
                        // Only process Press events to avoid duplicate triggers
                        if key_event.kind == event::KeyEventKind::Press {
                            tx.send(Event::Key(key_event)).expect("event send failed");
                        }
                    }
                }
                tx.send(Event::Tick).expect("tick send failed");
            }
        });

        EventHandler { rx }
    }

    pub fn next(&self) -> Result<Event> {
        Ok(self.rx.recv()?)
    }
}
