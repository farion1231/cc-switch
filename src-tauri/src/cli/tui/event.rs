use std::time::{Duration, Instant};

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};

pub enum Event {
    Tick,
    Key(KeyEvent),
    Resize(u16, u16),
}

pub struct EventHandler {
    tick_rate: Duration,
    last_tick: Instant,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        Self {
            tick_rate,
            last_tick: Instant::now(),
        }
    }

    pub fn next(&mut self) -> Result<Event, String> {
        let timeout = self
            .tick_rate
            .checked_sub(self.last_tick.elapsed())
            .unwrap_or(Duration::from_secs(0));

        if event::poll(timeout).map_err(|e| format!("Failed to poll terminal events: {e}"))? {
            match event::read().map_err(|e| format!("Failed to read terminal event: {e}"))? {
                CrosstermEvent::Key(key) => return Ok(Event::Key(key)),
                CrosstermEvent::Resize(cols, rows) => return Ok(Event::Resize(cols, rows)),
                _ => {}
            }
        }

        if self.last_tick.elapsed() >= self.tick_rate {
            self.last_tick = Instant::now();
            return Ok(Event::Tick);
        }

        Ok(Event::Tick)
    }
}
