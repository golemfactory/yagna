use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    process, thread,
    time::{Duration, Instant},
};

use clap::Parser;
use rand::{seq::SliceRandom, RngCore};
use serde_json::Value;
use tokio::task::JoinSet;

fn test_roundtrip(mib_per_s: f32, stream: &impl Fn() -> TcpStream) -> anyhow::Result<bool> {
    const MAX_TIME_SECS: f32 = 10.0;

    let mib: usize = (mib_per_s * MAX_TIME_SECS) as usize;
    let mut data = vec![0u8; mib * 1024 * 1024];
    rand::thread_rng().fill_bytes(&mut data);

    let mut stream = stream();

    let mut sender = stream.try_clone()?;
    let data_send = data.clone();
    let send_handle = thread::spawn(move || sender.write_all(&data_send));

    let mut data_recv = vec![0u8; data.len()];
    let read_handle = thread::spawn(move || -> anyhow::Result<_> {
        stream.read_exact(&mut data_recv)?;
        Ok(data_recv)
    });

    thread::sleep(Duration::from_secs_f32(MAX_TIME_SECS));

    if !read_handle.is_finished() {
        return Ok(false);
    }

    let recv_data = read_handle.join().unwrap()?;

    send_handle.join().unwrap()?;

    Ok(recv_data == data)
}

fn test_stress(mib_per_s: f32, stream: &impl Fn() -> TcpStream) -> anyhow::Result<bool> {
    let mut data = vec![0u8; (mib_per_s * 1024.0 * 1024.0) as usize * 8];
    rand::thread_rng().fill_bytes(&mut data);

    let mut stream = stream();

    let tries = 4;
    for _ in 0..tries {
        stream.write_all(&data)?;
        thread::sleep(Duration::from_secs(10));
    }

    Ok(true)
}

/// make iperf3 output conform to actual JSON
///
/// iperf3 --json actually outputs several json objects
/// concatenated, which will be (correctly) rejected by serde_json
/// due to trailing input. This function takes the first of the several
/// objects which contains everything we care about.
fn sanitize_iper3_output(text: &str) -> String {
    let mut first_json = String::new();
    let mut lines = text.lines();
    first_json.push_str(lines.next().unwrap());
    for line in lines {
        if line.starts_with('{') {
            break;
        } else {
            first_json.push_str(line);
        }
    }

    first_json
}

fn test_iperf3(mib_per_s: f32, host: &str, port: u16) -> anyhow::Result<bool> {
    let mut iperf3 = process::Command::new("iperf3")
        .arg("-p")
        .arg(port.to_string())
        .arg("--json")
        .arg("-c")
        .arg(host)
        .arg("-t")
        .arg("5")
        .arg("--logfile")
        .arg("iperf.log")
        .spawn()?;

    iperf3.wait()?;

    let log_text = std::fs::read_to_string("iperf.log")?;
    let json_text = sanitize_iper3_output(&log_text);

    let json: Value = serde_json::from_str(&json_text)?;

    let bits_per_second = move || -> Option<f64> {
        json.as_object()?
            .get("end")?
            .as_object()?
            .get("sum_sent")?
            .as_object()?
            .get("bits_per_second")?
            .as_f64()
    }()
    .ok_or(anyhow::anyhow!("malformed json"))?;

    Ok(bits_per_second as f32 >= mib_per_s * 1024.0 * 1024.0 * 8.0)
}

struct ResponseEntry {
    success: bool,
}

async fn run_request(url: String) -> ResponseEntry {
    eprintln!("Start request {}", &url);
    let elapsed = Instant::now();
    let res = reqwest::get(&url).await;

    match res {
        Ok(response) => {
            if response.status().is_success() {
                eprintln!(
                    "Success: {} {:.01}ms",
                    &url,
                    elapsed.elapsed().as_secs_f64() * 1000.0
                );
                ResponseEntry { success: true }
            } else {
                eprintln!(
                    "Http error: {} {:.01}ms {}",
                    &url,
                    elapsed.elapsed().as_secs_f64() * 1000.0,
                    response.status()
                );
                ResponseEntry { success: false }
            }
        }
        Err(err) => {
            eprintln!(
                "Request error: {} {:.01}ms {}",
                &url,
                elapsed.elapsed().as_secs_f64() * 1000.0,
                err
            );
            ResponseEntry { success: false }
        }
    }
}

fn test_many_reqs(
    address_list: &[String],
    total_reqs: usize,
    max_secs: f32,
) -> anyhow::Result<bool> {
    let mut requests_to_run = Vec::new();
    for i in 0..total_reqs {
        requests_to_run.push(address_list[i % address_list.len()].clone());
    }

    requests_to_run.shuffle(&mut rand::thread_rng());

    let started_at = Instant::now();
    let success_count = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async move {
            let mut set = JoinSet::new();

            for url in requests_to_run {
                set.spawn(run_request(url));
            }

            let mut results = vec![];
            while let Some(join_result) = set.join_next().await {
                let response: ResponseEntry = join_result.expect("task panicked");
                results.push(response);
            }

            results.iter().filter(|r| r.success).count()
        });

    Ok((success_count == total_reqs) && started_at.elapsed().as_secs_f32() < max_secs)
}

#[derive(Parser, Debug)]
#[command(author = "Golem Factory", version = "0.1.0", about = None, long_about = None)]
struct Args {
    #[arg(long, help = "host running the server", default_value_t = String::from("127.0.0.1"))]
    addr: String,

    #[arg(long, help = "port with the echo service", default_value_t = 2235)]
    port_echo: u16,

    #[arg(long, help = "port with the sink service", default_value_t = 2236)]
    port_sink: u16,

    #[arg(long, help = "port with the iperf3 service", default_value_t = 2237)]
    port_iperf: u16,

    #[arg(
        long,
        help = "throughput for the throughput, iperf3 and stress tests",
        default_value_t = 1.0
    )]
    mib_per_sec: f32,

    #[arg(
        long,
        help = "number of requests for the requests tests",
        default_value_t = 20
    )]
    requests_count: usize,

    #[arg(long, help = "only do first <stages> tests", default_value_t = 4)]
    stages: usize,

    #[arg(long, help = "list of external addresses to check")]
    address_list: Option<String>,

    #[arg(long, help = "file to write output json to")]
    output: Option<String>,
}

#[derive(serde::Serialize)]
struct Output {
    roundtrip: Result<bool, String>,
    many_reqs: Result<bool, String>,
    iperf3: Result<bool, String>,
    stress: Result<bool, String>,
}

fn main() {
    let Args {
        addr,
        port_echo,
        port_sink,
        port_iperf,
        mib_per_sec,
        requests_count,
        stages,
        address_list,
        output,
    } = Args::parse();

    let stream_echo = || TcpStream::connect(format!("{addr}:{port_echo}")).unwrap();
    let stream_sink = || TcpStream::connect(format!("{addr}:{port_sink}")).unwrap();

    let test_roundtrip_result = if stages >= 1 {
        let result = test_roundtrip(mib_per_sec, &stream_echo);
        eprintln!("{:?}", result);
        result
    } else {
        Err(anyhow::anyhow!("skipped"))
    };

    let addr_list = if let Some(address_list) = address_list {
        address_list
            .split(",")
            .map(|s| s.trim().to_string())
            .collect::<Vec<String>>()
    } else {
        Vec::from(
            [
                "http://ftp.cl.debian.org/debian/",
                "http://api.citybik.es/v2/networks",
                "http://ftp.au.debian.org/",
                "https://vanity.market/assets/logo_dark.svg",
            ]
            .map(|url| url.to_string()),
        )
    };

    let test_many_reqs_result = if stages >= 2 {
        let result = test_many_reqs(&addr_list, requests_count, requests_count as f32);
        eprintln!("{:?}", result);
        result
    } else {
        Err(anyhow::anyhow!("skipped"))
    };

    let test_iperf3_result = if stages >= 3 {
        let result = test_iperf3(mib_per_sec, &addr, port_iperf);
        eprintln!("{:?}", result);
        result
    } else {
        Err(anyhow::anyhow!("skipped"))
    };

    let test_stress_result = if stages >= 4 {
        let result = test_stress(mib_per_sec, &stream_sink);
        eprintln!("{:?}", result);
        result
    } else {
        Err(anyhow::anyhow!("skipped"))
    };

    let output_json = Output {
        roundtrip: test_roundtrip_result.map_err(|e| e.to_string()),
        many_reqs: test_many_reqs_result.map_err(|e| e.to_string()),
        iperf3: test_iperf3_result.map_err(|e| e.to_string()),
        stress: test_stress_result.map_err(|e| e.to_string()),
    };

    if let Some(output_file) = output {
        fs::write(output_file, serde_json::to_string(&output_json).unwrap()).unwrap();
    }
    println!("{}", serde_json::to_string(&output_json).unwrap());
}
