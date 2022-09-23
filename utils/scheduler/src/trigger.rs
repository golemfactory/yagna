use chrono::prelude::{DateTime, Local, TimeZone};
use chrono::Duration;
use std::fmt;

pub struct Interval {
    days: u32,
    hours: u32,
    minutes: u32,
    seconds: u32,
}

impl Interval {
    pub fn new(days: u32, hours: u32, minutes: u32, seconds: u32) -> Interval {
        Interval {
            days: days,
            hours: hours,
            minutes: minutes,
            seconds: seconds,
        }
    }

    fn next<Tz: TimeZone>(&self, from: DateTime<Tz>) -> DateTime<Tz> {
        from
            + Duration::days(i64::from(self.days))
            + Duration::hours(i64::from(self.hours))
            + Duration::minutes(i64::from(self.minutes))
            + Duration::seconds(i64::from(self.seconds))
    }
}

pub struct Trigger {
    name: String,
    interval: Interval,
    next_run: DateTime<Local>,
    last_run: Option<DateTime<Local>>,
}

impl Trigger {
    pub fn new<T>(name: T, start_from: DateTime<Local>, interval: Interval) -> Trigger
    where
        T: Into<String>,
    {
        Trigger {
            name: name.into(),
            interval: interval,
            next_run: start_from,
            last_run: None,
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
    use super::*;
    use chrono::Duration;
    use chrono::Local;

    #[test]
    fn test_interval_next_time() {
        let now = Local::now();
        let days = 1;
        let hours = 2;
        let minutes = 3;
        let seconds = 4;
        let interval = Interval::new(days, hours, minutes, seconds);
        let time_with_offset = now
            + Duration::days(i64::from(days))
            + Duration::hours(i64::from(hours))
            + Duration::minutes(i64::from(minutes))
            + Duration::seconds(i64::from(seconds));
        assert_eq!(interval.next(now), time_with_offset);
    }

    #[test]
    fn test_interval_next_time_strange_value() {
        let now = Local::now();
        let strange_seconds = 121;
        let seconds = strange_seconds % 60;
        let minutes = (strange_seconds / 60) as u32;
        let strange_interval = Interval::new(0, 0, 0, strange_seconds);
        let time_with_offset =
            now + Duration::minutes(i64::from(minutes)) + Duration::seconds(i64::from(seconds));
        assert_eq!(strange_interval.next(now), time_with_offset);
    }

    #[test]
    fn test_trigger_is_ready() {
        let seconds = 1;
        let interval = Interval::new(0, 0, 0, seconds);
        let trigger = Trigger::new("trigger1", Local::now(), interval);
        assert!(trigger.is_ready())
    }

    #[test]
    fn test_trigger_not_ready() {
        let days = 1;
        let interval = Interval::new(days, 0, 0, 0);
        let trigger = Trigger::new("trigger1", Local::now() + Duration::days(1), interval);
        assert!(!trigger.is_ready());
    }

    #[test]
    fn test_trigger_tick() {
        let days = 1;
        let now = Local::now();
        let interval = Interval::new(days, 0, 0, 0);
        let mut trigger = Trigger::new("trigger1", now.clone(), interval);
        trigger.tick();
        assert_ne!(trigger.last_run, None);
        assert_eq!(trigger.next_run, now + Duration::days(i64::from(days)));
    }
}
