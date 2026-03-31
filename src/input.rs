use crossterm::event::{self, Event as CEvent, KeyCode};
use tokio::sync::mpsc;


use crate::event::Event;

pub async fn input_task(tx: mpsc::Sender<Event>) {
    loop {
        if event::poll(std::time::Duration::from_millis(100)).unwrap() {
            if let CEvent::Key(key) = event::read().unwrap() {
                if let KeyCode::Char(c) = key.code {
                    if tx.send(Event::Key(c)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
