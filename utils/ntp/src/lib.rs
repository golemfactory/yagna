//! Startup sanity check for the local clock.
//!
//! Yagna's market, activity and payment flows all trust the wall clock: agreements
//! and invoices carry timestamps, events are ordered by them and REST clients poll
//! with `afterTimestamp`. A machine whose clock is off by minutes silently produces
//! offers nobody accepts and events nobody sees, with nothing in the logs pointing
//! at the cause. This module asks a handful of public NTP servers what time it is
//! and reports how far off we are.

use std::fmt;
use std::time::Duration;

use chrono::Local;
use futures::future::join_all;
use structopt::StructOpt;

mod sntp;

pub use sntp::{query, SntpSample, NTP_PORT};

/// Time services run by an open source project and by European public metrology
/// institutes, rather than by commercial anycast operators. Four independent
/// operators, so no single organisation can move the median on its own.
pub const DEFAULT_NTP_SERVERS: &[&str] = &[
    "ntp.ubuntu.com",  // Canonical
    "ptbtime1.ptb.de", // PTB, Germany
    "ntp.metas.ch",    // METAS, Switzerland
    "ntp1.inrim.it",   // INRIM, Italy
];

/// Offset above which the skew is logged as an error rather than as info.
pub const DEFAULT_MAX_OFFSET: Duration = Duration::from_millis(100);

#[derive(StructOpt, Clone, Debug)]
pub struct ClockCheckOpts {
    /// Skip the NTP clock check performed on startup [env: YAGNA_NO_CLOCK_CHECK]
    // `env` is not used on the flag: structopt would turn it into an argument that
    // requires a value, so the environment is read by `is_disabled`.
    #[structopt(long = "no-clock-check")]
    pub disabled: bool,

    /// NTP servers used by the clock check, comma separated.
    #[structopt(
        long = "ntp-servers",
        env = "YAGNA_NTP_SERVERS",
        use_delimiter = true,
        default_value = "ntp.ubuntu.com,ptbtime1.ptb.de,ntp.metas.ch,ntp1.inrim.it"
    )]
    pub servers: Vec<String>,

    /// Clock offset that turns the startup report into an error, in milliseconds.
    #[structopt(
        long = "max-clock-offset",
        env = "YAGNA_MAX_CLOCK_OFFSET",
        default_value = "100"
    )]
    pub max_offset_ms: u64,

    /// Time budget for a single NTP query, in seconds.
    #[structopt(
        long = "ntp-timeout",
        env = "YAGNA_NTP_TIMEOUT",
        default_value = "3",
        hidden = true
    )]
    pub timeout_secs: u64,
}

impl Default for ClockCheckOpts {
    fn default() -> Self {
        ClockCheckOpts {
            disabled: false,
            servers: DEFAULT_NTP_SERVERS.iter().map(|s| s.to_string()).collect(),
            max_offset_ms: DEFAULT_MAX_OFFSET.as_millis() as u64,
            timeout_secs: 3,
        }
    }
}

impl ClockCheckOpts {
    pub fn is_disabled(&self) -> bool {
        self.disabled || env_flag("YAGNA_NO_CLOCK_CHECK")
    }

    pub fn max_offset(&self) -> Duration {
        Duration::from_millis(self.max_offset_ms)
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Outcome of querying every configured server.
#[derive(Clone, Debug, Default)]
pub struct ClockCheck {
    /// Servers that answered.
    pub samples: Vec<SntpSample>,
    /// Servers that did not, with the reason.
    pub failures: Vec<(String, String)>,
}

impl ClockCheck {
    /// Median offset of the local clock in microseconds, positive when the local
    /// clock runs ahead. `None` when no server answered.
    ///
    /// The median is deliberate: a single lying or badly routed server cannot move
    /// it as long as most of the answers agree.
    pub fn offset_micros(&self) -> Option<i128> {
        if self.samples.is_empty() {
            return None;
        }
        let mut offsets: Vec<i128> = self.samples.iter().map(|s| s.offset_micros()).collect();
        offsets.sort_unstable();
        let mid = offsets.len() / 2;
        Some(if offsets.len() % 2 == 1 {
            offsets[mid]
        } else {
            // Round towards zero so an even split never reports more skew than seen.
            (offsets[mid - 1] + offsets[mid]) / 2
        })
    }

    /// True when the median offset exceeds `max_offset`. Unanswered checks are not
    /// failures: an offline node has no way to know, and must still be able to run.
    pub fn exceeds(&self, max_offset: Duration) -> bool {
        self.offset_micros()
            .map(|offset| offset.unsigned_abs() > max_offset.as_micros())
            .unwrap_or(false)
    }

    /// Classifies the offset against the configured threshold.
    pub fn verdict(&self, opts: &ClockCheckOpts) -> ClockVerdict {
        if self.offset_micros().is_none() {
            ClockVerdict::Unknown
        } else if self.exceeds(opts.max_offset()) {
            ClockVerdict::Alarming
        } else {
            ClockVerdict::Ok
        }
    }
}

impl fmt::Display for ClockCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.offset_micros() {
            Some(offset) => write!(
                f,
                "local clock is {}, according to {} of {} NTP server(s)",
                format_offset(offset),
                self.samples.len(),
                self.samples.len() + self.failures.len(),
            ),
            None => write!(f, "no NTP server answered ({} tried)", self.failures.len()),
        }
    }
}

/// What the measured offset means. Nothing here stops startup: a node with a bad
/// clock is still better off running and saying so loudly than refusing to boot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockVerdict {
    /// No server answered, so there is nothing to judge.
    Unknown,
    /// Within the threshold: reported at info.
    Ok,
    /// Past the threshold: reported at error level, but yagna keeps going.
    Alarming,
}

/// Local time and time zone, as the machine sees them.
///
/// Printed next to the offset because the two failure modes look alike in logs and
/// bug reports: a clock that is genuinely wrong, and a clock that is right but read
/// in an unexpected zone.
pub fn local_time_description() -> String {
    let now = Local::now();
    match time_zone_name() {
        Some(name) => format!("{} ({})", now.format("%Y-%m-%d %H:%M:%S%.3f %:z"), name),
        None => now.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string(),
    }
}

/// Best effort IANA time zone name. There is no portable API for it, so this reads
/// the usual places and simply gives up when none of them says anything.
fn time_zone_name() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim().trim_start_matches(':').to_string();
        if !tz.is_empty() {
            return Some(tz);
        }
    }
    #[cfg(unix)]
    {
        if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
            let tz = tz.trim().to_string();
            if !tz.is_empty() {
                return Some(tz);
            }
        }
        if let Ok(link) = std::fs::read_link("/etc/localtime") {
            let path = link.to_string_lossy();
            if let Some((_, name)) = path.split_once("zoneinfo/") {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Reads a boolean environment variable. Anything but `0`, `false`, `no` or an
/// empty value turns the flag on.
fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        ),
        Err(_) => false,
    }
}

/// Formats a signed microsecond offset the way a human reads it.
pub fn format_offset(micros: i128) -> String {
    let magnitude = micros.unsigned_abs();
    let direction = if micros < 0 { "behind" } else { "ahead" };
    let value = if magnitude < 1_000 {
        format!("{magnitude}us")
    } else if magnitude < 1_000_000 {
        format!("{:.1}ms", magnitude as f64 / 1_000.)
    } else {
        format!("{:.3}s", magnitude as f64 / 1_000_000.)
    };
    format!("{value} {direction}")
}

/// Queries all configured servers concurrently. Never fails as a whole: servers
/// that time out or misbehave end up in [`ClockCheck::failures`].
pub async fn check_clock(opts: &ClockCheckOpts) -> ClockCheck {
    let timeout = opts.timeout();
    let results = join_all(
        opts.servers
            .iter()
            .map(|server| async move { (server.clone(), sntp::query(server, timeout).await) }),
    )
    .await;

    let mut check = ClockCheck::default();
    for (server, result) in results {
        match result {
            Ok(sample) => {
                log::debug!(
                    "NTP {} ({}): {}, round trip {:?}",
                    sample.server,
                    sample.addr,
                    format_offset(sample.offset_micros()),
                    sample.delay,
                );
                check.samples.push(sample);
            }
            Err(e) => {
                log::debug!("NTP {server} did not answer: {e:#}");
                check.failures.push((server, format!("{e:#}")));
            }
        }
    }
    check
}

/// Runs the check and reports the skew: at info while the offset stays below
/// `--max-clock-offset`, at error level above it. Startup is never stopped, so
/// this returns no error - a node with a bad clock is more useful running and
/// complaining than refusing to boot.
pub async fn check_clock_on_startup(opts: &ClockCheckOpts) -> ClockCheck {
    if opts.is_disabled() {
        log::debug!("Clock check disabled");
        return ClockCheck::default();
    }

    let check = check_clock(opts).await;
    let local_time = local_time_description();

    match check.verdict(opts) {
        ClockVerdict::Unknown => {
            log::info!("Local time: {local_time}. {check}, so it could not be verified.");
        }
        ClockVerdict::Ok => log::info!("Local time: {local_time}. Clock check: {check}."),
        ClockVerdict::Alarming => {
            log::error!(
                "CLOCK OUT OF SYNC: {check} - more than the {}ms this node should ever drift. \
                 Local time: {local_time}. Golem orders agreements, activity events and payments \
                 by wall clock timestamps, so a clock this far off causes failures that are very \
                 hard to diagnose: offers that nobody accepts, events that nobody sees, invoices \
                 rejected as stale. Enable time synchronization (NTP) on this machine. Yagna keeps \
                 running, but expect trouble until the clock is fixed.",
                opts.max_offset_ms,
            );
        }
    }

    check
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn sample(offset_micros: i128) -> SntpSample {
        let addr: SocketAddr = ([127, 0, 0, 1], NTP_PORT).into();
        SntpSample {
            server: "test".into(),
            addr,
            offset: Duration::from_micros(offset_micros.unsigned_abs() as u64),
            offset_is_negative: offset_micros < 0,
            delay: Duration::from_millis(10),
            stratum: 2,
        }
    }

    fn check_of(offsets: &[i128]) -> ClockCheck {
        ClockCheck {
            samples: offsets.iter().map(|o| sample(*o)).collect(),
            failures: vec![],
        }
    }

    #[test]
    fn no_samples_means_no_verdict() {
        let check = ClockCheck::default();
        assert_eq!(check.offset_micros(), None);
        assert!(!check.exceeds(DEFAULT_MAX_OFFSET));
    }

    #[test]
    fn median_ignores_a_single_outlier() {
        let check = check_of(&[1_000, 2_000, 900_000_000]);
        assert_eq!(check.offset_micros(), Some(2_000));
        assert!(!check.exceeds(DEFAULT_MAX_OFFSET));
    }

    #[test]
    fn the_threshold_splits_info_from_alarm() {
        let opts = ClockCheckOpts::default();
        assert_eq!(check_of(&[50_000]).verdict(&opts), ClockVerdict::Ok);
        assert_eq!(check_of(&[-99_000]).verdict(&opts), ClockVerdict::Ok);
        // The threshold is inclusive: exactly 100ms is still only info.
        assert_eq!(check_of(&[100_000]).verdict(&opts), ClockVerdict::Ok);
        assert_eq!(check_of(&[100_001]).verdict(&opts), ClockVerdict::Alarming);
        assert_eq!(check_of(&[-100_001]).verdict(&opts), ClockVerdict::Alarming);
        assert_eq!(ClockCheck::default().verdict(&opts), ClockVerdict::Unknown);
    }

    #[test]
    fn the_local_time_line_carries_a_zone() {
        let description = local_time_description();
        assert!(
            description.contains('+') || description.contains('-'),
            "no UTC offset in {:?}",
            description
        );
    }

    #[test]
    fn even_sample_count_averages_the_middle_pair() {
        assert_eq!(check_of(&[1_000, 3_000]).offset_micros(), Some(2_000));
    }

    #[test]
    fn negative_offsets_are_compared_by_magnitude() {
        let check = check_of(&[-9_000_000, -9_100_000, -9_200_000]);
        assert_eq!(check.offset_micros(), Some(-9_100_000));
        assert!(check.exceeds(DEFAULT_MAX_OFFSET));
        assert!(!check.exceeds(Duration::from_secs(30)));
    }

    #[test]
    fn offsets_are_formatted_with_a_direction() {
        assert_eq!(format_offset(-2_500_000), "2.500s behind");
        assert_eq!(format_offset(1_500), "1.5ms ahead");
        assert_eq!(format_offset(12), "12us ahead");
    }

    #[tokio::test]
    async fn a_disabled_check_does_not_touch_the_network() {
        let opts = ClockCheckOpts {
            disabled: true,
            servers: vec!["192.0.2.1".into()], // TEST-NET-1, never answers
            timeout_secs: 30,
            ..Default::default()
        };
        let check = check_clock_on_startup(&opts).await;
        assert!(check.samples.is_empty() && check.failures.is_empty());
    }

    #[tokio::test]
    async fn a_wildly_wrong_clock_never_stops_startup() {
        // 192.0.2.1 is TEST-NET-1: nothing answers, so the check stays inconclusive
        // and still returns rather than failing.
        let opts = ClockCheckOpts {
            servers: vec!["192.0.2.1".into()],
            timeout_secs: 1,
            ..Default::default()
        };
        assert_eq!(
            check_clock_on_startup(&opts).await.verdict(&opts),
            ClockVerdict::Unknown
        );
    }

    #[tokio::test]
    async fn unreachable_servers_are_reported_as_failures() {
        let opts = ClockCheckOpts {
            servers: vec!["192.0.2.1".into()],
            timeout_secs: 1,
            ..Default::default()
        };
        let check = check_clock(&opts).await;
        assert!(check.samples.is_empty());
        assert_eq!(check.failures.len(), 1);
    }
}

/// Hits the real network; run with `cargo test -p ya-ntp -- --ignored`.
#[cfg(test)]
mod network_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn public_servers_answer_with_a_sane_offset() {
        let check = check_clock(&ClockCheckOpts::default()).await;
        println!("{check}");
        for sample in &check.samples {
            println!(
                "{} ({}) stratum {}: {}, round trip {:?}",
                sample.server,
                sample.addr,
                sample.stratum,
                format_offset(sample.offset_micros()),
                sample.delay
            );
        }
        for (server, error) in &check.failures {
            println!("{server}: {error}");
        }
        assert!(!check.samples.is_empty(), "no NTP server answered");
    }
}
