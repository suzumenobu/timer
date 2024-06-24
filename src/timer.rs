use std::{
    borrow::BorrowMut,
    fmt::Display,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    self,
    sync::mpsc::{error::TryRecvError, Receiver},
};

#[derive(Debug)]
pub struct Error;
pub type Result<T> = std::result::Result<T, Error>;

pub struct CurrentTime(Duration);

impl Display for CurrentTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let minutes = (self.0.as_secs() / 60) as u8;
        let minutes = if minutes < 10 {
            format!("0{minutes}")
        } else {
            minutes.to_string()
        };

        let seconds = self.0.as_secs() % 60;
        let seconds = if seconds < 10 {
            format!("0{seconds}")
        } else {
            seconds.to_string()
        };

        write!(f, "{minutes}:{seconds}")
    }
}

pub enum TimerEvent {
    Start { left: CurrentTime },
    Tick { left: CurrentTime },
    Finish,
}

pub struct TimerConfig {
    pub amount: Duration,
    pub tick: Duration,
}

pub trait TimerEventHandler
where
    Self: Sync + Send + 'static,
{
    fn handle(&self, event: TimerEvent) -> Result<()>;
}

#[derive(Clone)]
pub enum TimerCommand {
    Stop,
}

pub async fn countdown(
    event_handler: impl TimerEventHandler,
    config: TimerConfig,
    rx: Arc<Mutex<Receiver<TimerCommand>>>,
) {
    let ticks_count = config.amount.as_secs() / config.tick.as_secs();
    event_handler
        .handle(TimerEvent::Start {
            left: CurrentTime(config.amount.clone()),
        })
        .unwrap();

    for idx in 0..ticks_count {
        tokio::time::sleep(config.tick).await;
        event_handler
            .handle(TimerEvent::Tick {
                left: CurrentTime(
                    config
                        .amount
                        .checked_sub(config.tick.checked_mul(idx as u32).unwrap())
                        .unwrap(),
                ),
            })
            .unwrap();

        let mut rx = rx.lock().unwrap();
        match rx.borrow_mut().try_recv() {
            Ok(cmd) => match cmd {
                TimerCommand::Stop => break,
            },
            Err(err) => match err {
                TryRecvError::Disconnected => break,
                _ => continue,
            },
        }
    }

    event_handler.handle(TimerEvent::Finish).unwrap();
}
