use chrono::prelude::*;
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

pub static SYSTEM_TIME : SystemTime = SystemTime(()); 

#[cfg(test)]
pub struct FakeTime {
    now: DateTime<Utc>
}

#[cfg(test)]
impl WallClock for FakeTime {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }
}

#[cfg(test)]
impl FakeTime {
    pub fn new(now: DateTime<Utc>) -> Self {
        FakeTime { now }
    }

    pub fn advance(self, dur: Duration) -> Self {
        FakeTime { now: self.now + dur }
    }
}
