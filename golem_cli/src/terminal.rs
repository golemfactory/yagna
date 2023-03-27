use crossterm::style::Stylize;
use crossterm::*;

use crossterm::tty::IsTty;
use std::cmp::{max, min};
use std::io::{stderr, Write};
use tokio::time::Duration;

fn pack_into<F: FnMut() -> u8>(buf: &mut [u8], f: &mut F) {
    for ch in buf.iter_mut() {
        *ch = f()
    }
}

#[inline]
fn safe_split(s: &str, offset: usize) -> &str {
    if offset < s.len() {
        &s[offset..]
    } else {
        ""
    }
}

fn find_size(banner: &str) -> (usize, usize) {
    banner
        .lines()
        .fold((0, 0), |(w, h), line| (max(w, line.len()), h + 1))
}

pub fn fade_in(banner: &str) -> anyhow::Result<()> {
    let mut stderr = stderr();

    if !stderr.is_tty() {
        eprintln!("{}", banner);
        return Ok(());
    }
    let (w, h) = crossterm::terminal::size()?;
    let (bw, bh) = find_size(banner);
    if (w as usize) < bw + 2 || (h as usize) < bh + 1 {
        eprintln!("{}", banner);
        return Ok(());
    }

    let noise = b"@Oo*.";
    let mut seed = 1000usize;
    let mut noise_buf = [b' '; 5];
    let mut next_noise_char = move || -> u8 {
        let n = seed % noise.len();
        seed = (seed * 13 + 7) & 0xFFFF;
        noise[n]
    };

    queue!(stderr, cursor::Hide)?;
    for frame in 0.. {
        let mut nlines = 0;
        let mut next_frame: bool = false;
        for line in banner.lines() {
            let offset = if 5 + (frame * 2 / 3) > nlines {
                5 + (frame * 2 / 3) - nlines
            } else {
                0
            };

            let (pre, post) = if line.len() > offset {
                next_frame = true;

                (&line[..offset], &line[offset..min(line.len(), w as usize)])
            } else {
                (line, "")
            };

            let post = if post.is_empty() {
                ("", "")
            } else {
                pack_into(noise_buf.as_mut(), &mut next_noise_char);
                let noise = std::str::from_utf8(&noise_buf[..min(noise_buf.len(), post.len())])?;
                (noise, safe_split(post, noise.len()))
            };
            queue!(
                stderr,
                style::Print(pre),
                style::PrintStyledContent(post.0.red()),
                style::PrintStyledContent(post.1.black()),
                style::Print("\n")
            )?;
            nlines += 1;
        }
        stderr.flush()?;
        if next_frame {
            queue!(stderr, cursor::MoveToPreviousLine(nlines as u16))?;
        } else {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    queue!(stderr, cursor::Show)?;
    stderr.flush()?;
    Ok(())
}

pub async fn clear_stdin() -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    while crossterm::event::poll(Duration::from_millis(100))? {
        let _ = crossterm::event::read()?;
    }
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
