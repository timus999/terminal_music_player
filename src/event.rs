use std::time::Duration;
use tokio::sync::mpsc;


#[derive(Debug)]
pub enum Event {
    Tick,
    Key(char),
}

pub fn event_channel() -> (mpsc::Sender<Event>, mpsc::Receiver<Event>) {
    mpsc::channel(100)
}


pub async fn tick_task(tx: mpsc::Sender<Event>) {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    loop {
        interval.tick().await;
        if tx.send(Event::Tick).await.is_err() {
            break;
        }
    }
}
