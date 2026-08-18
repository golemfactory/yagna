//! Minimal SNTP (RFC 4330) client, just enough to ask a server "what time is it?"
//! and compute the offset of the local clock against the answer.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use rand::Rng;
use tokio::net::UdpSocket;

pub const NTP_PORT: u16 = 123;

const PACKET_LEN: usize = 48;
/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch (1970-01-01).
const NTP_UNIX_OFFSET: i128 = 2_208_988_800;
/// Leap indicator 0, version 4, mode 3 (client).
const LI_VN_MODE_CLIENT: u8 = 0b00_100_011;
const MODE_SERVER: u8 = 4;

/// Result of a single successful query.
#[derive(Clone, Debug)]
pub struct SntpSample {
    /// Server the sample came from, as given in the configuration.
    pub server: String,
    pub addr: SocketAddr,
    /// How far the local clock is ahead of the server. Negative means we are behind.
    pub offset: Duration,
    pub offset_is_negative: bool,
    /// Round trip delay of the exchange.
    pub delay: Duration,
    pub stratum: u8,
}

impl SntpSample {
    /// Signed offset in microseconds; positive when the local clock runs ahead.
    pub fn offset_micros(&self) -> i128 {
        let micros = self.offset.as_micros() as i128;
        if self.offset_is_negative {
            -micros
        } else {
            micros
        }
    }

    fn from_micros(
        server: String,
        addr: SocketAddr,
        offset: i128,
        delay: i128,
        stratum: u8,
    ) -> Self {
        SntpSample {
            server,
            addr,
            offset: Duration::from_micros(offset.unsigned_abs().min(u64::MAX as u128) as u64),
            offset_is_negative: offset < 0,
            delay: Duration::from_micros(delay.max(0).min(u64::MAX as i128) as u64),
            stratum,
        }
    }
}

/// Queries a single server. `server` may be a host name or an IP address, with an
/// optional `:port` suffix; port 123 is used when none is given.
pub async fn query(server: &str, timeout: Duration) -> Result<SntpSample> {
    let addr = resolve(server)
        .await
        .with_context(|| format!("resolving NTP server {server}"))?;

    let bind: SocketAddr = match addr {
        SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = UdpSocket::bind(bind)
        .await
        .context("binding a local UDP socket")?;
    socket
        .connect(addr)
        .await
        .with_context(|| format!("connecting to {addr}"))?;

    // The transmit timestamp doubles as a nonce: the server has to echo it back
    // verbatim, which makes off-path answers easy to reject. Only the low bits of
    // the fraction are randomized, so the value stays a valid reading of our clock.
    let t1_raw = now_ntp()? | rand::thread_rng().gen::<u16>() as u64;
    let mut request = [0u8; PACKET_LEN];
    request[0] = LI_VN_MODE_CLIENT;
    request[40..48].copy_from_slice(&t1_raw.to_be_bytes());

    let response = tokio::time::timeout(timeout, async {
        socket.send(&request).await.context("sending the request")?;
        loop {
            let mut buf = [0u8; PACKET_LEN + 16];
            let len = socket.recv(&mut buf).await.context("awaiting the reply")?;
            if len < PACKET_LEN {
                // Runt packet; keep waiting until the timeout fires.
                continue;
            }
            // Read our own clock as early as possible after the reply lands.
            let t4 = now_micros()?;
            let mut packet = [0u8; PACKET_LEN];
            packet.copy_from_slice(&buf[..PACKET_LEN]);
            if be_u64(&packet[24..32]) != t1_raw {
                // Not an answer to our request.
                continue;
            }
            return Ok::<_, anyhow::Error>((packet, t4));
        }
    })
    .await
    .map_err(|_| anyhow!("no reply within {timeout:?}"))??;

    let (packet, t4) = response;
    let mode = packet[0] & 0b111;
    if mode != MODE_SERVER {
        bail!("unexpected mode {mode} in the reply");
    }
    let stratum = packet[1];
    if stratum == 0 {
        // Kiss-o'-death: the server is telling us to go away, its timestamps are unusable.
        let code = String::from_utf8_lossy(&packet[12..16])
            .trim_end()
            .to_string();
        bail!("server refused to answer (kiss-o'-death code {code:?})");
    }
    if stratum > 15 {
        bail!("server is unsynchronized (stratum {stratum})");
    }

    let t1 = ntp_to_micros(t1_raw);
    let t2 = ntp_to_micros(be_u64(&packet[32..40]));
    let t3 = ntp_to_micros(be_u64(&packet[40..48]));

    let offset = ((t1 - t2) + (t4 - t3)) / 2;
    let delay = (t4 - t1) - (t3 - t2);

    Ok(SntpSample::from_micros(
        server.to_string(),
        addr,
        offset,
        delay,
        stratum,
    ))
}

async fn resolve(server: &str) -> Result<SocketAddr> {
    let with_port = if server.parse::<IpAddr>().is_ok() {
        // Bare IPv6 literals need bracketing before a port can be appended.
        format!("{server}:{NTP_PORT}")
    } else if server
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .is_some()
        && server.matches(':').count() == 1
    {
        server.to_string()
    } else {
        format!("{server}:{NTP_PORT}")
    };

    let resolved = tokio::net::lookup_host(&with_port)
        .await
        .with_context(|| format!("looking up {with_port}"))?
        .next();
    resolved.ok_or_else(|| anyhow!("{server} resolved to no addresses"))
}

/// Reads a big endian `u64` out of an 8 byte slice.
fn be_u64(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(bytes);
    u64::from_be_bytes(buf)
}

/// Local wall clock as microseconds since the Unix epoch.
fn now_micros() -> Result<i128> {
    let now = SystemTime::now();
    Ok(match now.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_micros() as i128,
        Err(e) => -(e.duration().as_micros() as i128),
    })
}

/// Local wall clock as a 64 bit NTP timestamp (32.32 fixed point seconds since 1900).
fn now_ntp() -> Result<u64> {
    micros_to_ntp(now_micros()?)
}

/// Microseconds since the Unix epoch as a 64 bit NTP timestamp.
fn micros_to_ntp(micros: i128) -> Result<u64> {
    let secs = micros.div_euclid(1_000_000) + NTP_UNIX_OFFSET;
    let frac_micros = micros.rem_euclid(1_000_000);
    if !(0..=u32::MAX as i128).contains(&secs) {
        bail!("the local clock is outside of the range representable by NTP");
    }
    let frac = (frac_micros << 32) / 1_000_000;
    Ok(((secs as u64) << 32) | frac as u64)
}

/// Builds a server reply to `request`, claiming the time is `shift_micros` away
/// from this machine's clock. Test helper, but it lives here so it stays next to
/// the packet layout it mirrors.
#[cfg(test)]
fn fake_reply(request: &[u8], shift_micros: i128, stratum: u8) -> Result<[u8; PACKET_LEN]> {
    let mut reply = [0u8; PACKET_LEN];
    reply[0] = 0b00_100_100; // leap 0, version 4, mode 4 (server)
    reply[1] = stratum;
    reply[24..32].copy_from_slice(&request[40..48]); // originate = client transmit
    let now = micros_to_ntp(now_micros()? + shift_micros)?.to_be_bytes();
    reply[32..40].copy_from_slice(&now); // receive
    reply[40..48].copy_from_slice(&now); // transmit
    Ok(reply)
}

/// NTP timestamp to microseconds since the Unix epoch.
fn ntp_to_micros(raw: u64) -> i128 {
    let secs = (raw >> 32) as i128 - NTP_UNIX_OFFSET;
    let frac = (raw & 0xffff_ffff) as i128;
    secs * 1_000_000 + ((frac * 1_000_000) >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves NTP on localhost, always claiming to be `shift_micros` away from us.
    async fn spawn_fake_server(shift_micros: i128, stratum: u8) -> SocketAddr {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 128];
            while let Ok((len, peer)) = socket.recv_from(&mut buf).await {
                if len < PACKET_LEN {
                    continue;
                }
                let reply = fake_reply(&buf[..PACKET_LEN], shift_micros, stratum).unwrap();
                let _ = socket.send_to(&reply, peer).await;
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_shifted_server_shows_up_as_the_opposite_offset() {
        // The server is two seconds ahead of us, so our clock is two seconds behind.
        let addr = spawn_fake_server(2_000_000, 2).await;
        let sample = query(&addr.to_string(), Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(sample.stratum, 2);
        let offset = sample.offset_micros();
        assert!(
            (-2_100_000..-1_900_000).contains(&offset),
            "offset was {}us",
            offset
        );
    }

    #[tokio::test]
    async fn a_synchronized_server_shows_up_as_no_offset() {
        let addr = spawn_fake_server(0, 1).await;
        let sample = query(&addr.to_string(), Duration::from_secs(2))
            .await
            .unwrap();
        assert!(
            sample.offset < Duration::from_millis(50),
            "offset was {:?}",
            sample.offset
        );
    }

    #[tokio::test]
    async fn a_kiss_of_death_reply_is_rejected() {
        let addr = spawn_fake_server(0, 0).await;
        let error = query(&addr.to_string(), Duration::from_secs(2))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("kiss-o'-death"), "error was {}", error);
    }

    #[test]
    fn ntp_and_unix_epochs_round_trip() {
        let ntp = now_ntp().unwrap();
        let diff = (ntp_to_micros(ntp) - now_micros().unwrap()).abs();
        // Conversion loses at most a microsecond; the rest is the time between the reads.
        assert!(diff < 1_000_000, "diff was {}us", diff);
    }

    #[test]
    fn unix_epoch_maps_to_the_ntp_epoch_offset() {
        assert_eq!(ntp_to_micros((NTP_UNIX_OFFSET as u64) << 32), 0);
    }

    #[test]
    fn fractions_are_converted() {
        // Half a second in 32.32 fixed point.
        let raw = ((NTP_UNIX_OFFSET as u64) << 32) | 0x8000_0000;
        assert_eq!(ntp_to_micros(raw), 500_000);
    }

    #[test]
    fn sample_offset_keeps_its_sign() {
        let addr: SocketAddr = ([127, 0, 0, 1], NTP_PORT).into();
        let ahead = SntpSample::from_micros("s".into(), addr, 1_500_000, 10, 2);
        let behind = SntpSample::from_micros("s".into(), addr, -1_500_000, 10, 2);
        assert_eq!(ahead.offset_micros(), 1_500_000);
        assert_eq!(behind.offset_micros(), -1_500_000);
        assert_eq!(behind.offset, Duration::from_millis(1500));
    }
}
