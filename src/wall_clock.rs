use chrono::prelude::*;
#[cfg(test)]
use chrono::Duration;

pub trait WallClock {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Copy, Clone)]
pub struct SystemTime(());

impl WallClock for SystemTime {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub static SYSTEM_TIME: SystemTime = SystemTime(());

#[cfg(test)]
pub struct FakeTime {
    current: DateTime<Utc>,
}

#[cfg(test)]
impl WallClock for FakeTime {
    fn now(&self) -> DateTime<Utc> {
        self.current
    }
}

#[cfg(test)]
impl FakeTime {
    pub fn new(current: DateTime<Utc>) -> Self {
        FakeTime { current }
    }

    pub fn advance(self, dur: Duration) -> Self {
        FakeTime {
            current: self.current + dur,
        }
    }
}
