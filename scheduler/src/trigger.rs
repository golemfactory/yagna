use chrono::prelude::{DateTime, Local, TimeZone};
use chrono::Duration;
use std::fmt;

pub struct Interval {
    seconds: u32,
    minutes: u32,
    hours: u32,
    days: u32,
}

pub trait NextTime {
    fn next<Tz: TimeZone>(&self, from: DateTime<Tz>) -> DateTime<Tz>;
}

impl NextTime for Interval {
    fn next<Tz: TimeZone>(&self, from: DateTime<Tz>) -> DateTime<Tz> {
        from.clone()
            + Duration::days(i64::from(self.days))
            + Duration::hours(i64::from(self.hours))
            + Duration::minutes(i64::from(self.minutes))
            + Duration::seconds(i64::from(self.seconds))
    }
}

impl Interval {
    pub fn new(days: u32, hours: u32, minutes: u32, seconds: u32) -> Interval {
        Interval {
            seconds: seconds,
            minutes: minutes,
            hours: hours,
            days: days,
        }
    }
}

pub struct Trigger {
    name: String,
    interval: Interval,
    next_run: DateTime<Local>,
    last_run: Option<DateTime<Local>>,
}

impl Trigger {
    pub fn new(name: String, start_from: DateTime<Local>, interval: Interval) -> Trigger {
        Trigger {
            name: name,
            next_run: interval.next(start_from),
            last_run: None,
            interval: interval,
        }
    }

    pub fn tick(&mut self) {
        let now = Local::now();
        self.next_run = self.interval.next(self.next_run);
        self.last_run = Some(now);
    }

    pub fn is_ready(&self) -> bool {
        self.next_run <= Local::now()
    }
}

impl fmt::Debug for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Trigger {{ name: {:?}, next_run: {:?}, last_run: {:?} }}",
            self.name, self.next_run, self.last_run
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
