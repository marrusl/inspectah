use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};

/// Events the TUI processes.
#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
}

/// Polls crossterm events in a background thread, sends to main loop.
pub struct EventReader {
    rx: mpsc::Receiver<Event>,
    _handle: thread::JoinHandle<()>,
}

impl EventReader {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let send = |evt| tx.send(evt).is_ok();
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    let ok = match event::read() {
                        Ok(CrosstermEvent::Key(key)) => send(Event::Key(key)),
                        Ok(CrosstermEvent::Resize(w, h)) => send(Event::Resize(w, h)),
                        _ => true,
                    };
                    if !ok {
                        break;
                    }
                } else {
                    // Tick on timeout — drives flash message expiry.
                    if !send(Event::Tick) {
                        break;
                    }
                }
            }
        });

        Self {
            rx,
            _handle: handle,
        }
    }

    /// Blocking receive. Returns None when sender is dropped.
    pub fn next(&self) -> Option<Event> {
        self.rx.recv().ok()
    }
}
