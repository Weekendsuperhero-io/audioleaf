use crate::{constants, panic};
use anyhow::Result;
use ratatui::crossterm::event;
use std::{sync::mpsc, thread, time::Duration};

pub struct EventHandler {
    rx: mpsc::Receiver<event::KeyEvent>,
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
                    if let event::Event::Key(key) = event {
                        if key.kind == event::KeyEventKind::Press {
                            tx.send(key).expect("event send failed");
                        }
                    }
                }
            }
        });

        EventHandler { rx }
    }

    pub fn next(&self) -> Result<event::KeyEvent> {
        Ok(self.rx.recv()?)
    }
}
