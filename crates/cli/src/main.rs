use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use bdownloader_core::{Checksum, DownloadOptions, Engine, MAX_CONNECTIONS, Outcome, Progress};

const USAGE: &str = "\
Usage: bdl <URL> [options]

Options:
  -o, --dir DIR           folder to save into (default: current folder)
  -n, --name NAME         file name to save as (default: from the server)
  -c, --connections N     parallel connections, 1-64 (default: 8)
      --sha256 HEX        verify the finished file's SHA-256
      --md5 HEX           verify the finished file's MD5
  -H, --header \"N: V\"   send this request header too (repeatable), e.g. a Cookie
  -h, --help              show this help

Press Ctrl+C to pause. Run the same command again to resume.";

#[tokio::main]
async fn main() -> ExitCode {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let engine = match Engine::new() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let handle = engine.start(options);
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let mut paused = false;
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    let mut speed = Speed::new();
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                print_progress(&handle.progress(), &mut speed);
                if handle.is_finished() {
                    break;
                }
            }
            _ = &mut ctrl_c, if !paused => {
                paused = true;
                handle.pause();
            }
        }
    }
    println!();

    match handle.wait().await {
        Ok(Outcome::Completed(path)) => {
            println!("Saved to {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::Paused) => {
            println!("Paused. Run the same command again to resume.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<DownloadOptions>, String> {
    let mut url = None;
    let mut options = DownloadOptions::new("", ".");
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "-o" | "--dir" => options.dir = PathBuf::from(value(&arg)?),
            "-n" | "--name" => options.file_name = Some(value(&arg)?),
            "-c" | "--connections" => {
                let n: usize = value(&arg)?.parse().map_err(|_| "--connections needs a number".to_owned())?;
                if !(1..=MAX_CONNECTIONS).contains(&n) {
                    return Err(format!("--connections must be between 1 and {MAX_CONNECTIONS}"));
                }
                options.connections = n;
            }
            "--sha256" => options.checksum = Some(Checksum::Sha256(value(&arg)?)),
            "--md5" => options.checksum = Some(Checksum::Md5(value(&arg)?)),
            "-H" | "--header" => {
                let header = value(&arg)?;
                let (name, value) = header.split_once(':').ok_or("--header needs \"Name: value\"")?;
                options.headers.push((name.trim().to_owned(), value.trim().to_owned()));
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a}")),
            a if url.is_none() => url = Some(a.to_owned()),
            a => return Err(format!("unexpected argument {a}")),
        }
    }
    options.url = url.ok_or("missing URL")?;
    Ok(Some(options))
}

/// Download speed averaged over the last few seconds.
struct Speed {
    samples: Vec<(Instant, u64)>,
}

impl Speed {
    const WINDOW: Duration = Duration::from_secs(3);

    fn new() -> Self {
        Self { samples: Vec::new() }
    }

    fn update(&mut self, downloaded: u64) -> f64 {
        let now = Instant::now();
        self.samples.push((now, downloaded));
        self.samples.retain(|(t, _)| now.duration_since(*t) <= Self::WINDOW);
        let (t0, b0) = self.samples[0];
        let secs = now.duration_since(t0).as_secs_f64();
        if secs > 0.0 { downloaded.saturating_sub(b0) as f64 / secs } else { 0.0 }
    }
}

fn print_progress(p: &Progress, speed: &mut Speed) {
    let rate = speed.update(p.downloaded);
    let done = match p.total {
        Some(total) if total > 0 => format!(
            "{:5.1}%  {} / {}",
            p.downloaded as f64 * 100.0 / total as f64,
            size(p.downloaded as f64),
            size(total as f64)
        ),
        _ => size(p.downloaded as f64),
    };
    let line = format!("{done}  {}/s  {} conn  {:?}", size(rate), p.active_connections, p.status);
    print!("\r{line:<79}");
    let _ = std::io::stdout().flush();
}

fn size(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
