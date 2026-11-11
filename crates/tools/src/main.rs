//! `upwind`: builds the wind history and traces air through it.

mod gfs;
mod ingest;
mod synth;
mod time;
mod timeline;
mod trace;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use gfs::Result;

const USAGE: &str = "\
upwind — where has the air been?

  upwind ingest [--data DIR] [--days 10] [--jobs 4] [--now CYCLE]
      Download the newest wind from NOAA's GFS model into DIR and drop
      what has aged out. Run it again at any time; it only fetches what
      is missing.

  upwind trace --lat DEG --lon DEG [--data DIR] [--height 500[,1500,...]]
               [--hours 240] [--time CYCLE] [--every 6] [--json]
      Follow the air arriving above a place back through the last days.

  upwind synth [--data DIR]
      Write a small made-up wind history for tests.

DIR defaults to ./data. CYCLE is a UTC hour written YYYYMMDDHH.";

struct Args {
    flags: HashMap<String, String>,
}

impl Args {
    fn parse(raw: &[String]) -> Result<Self> {
        let mut flags = HashMap::new();
        let mut i = 0;
        while i < raw.len() {
            let name = raw[i]
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument '{}'", raw[i]))?;
            if name == "json" {
                flags.insert(name.to_string(), String::new());
                i += 1;
            } else {
                let value = raw
                    .get(i + 1)
                    .ok_or_else(|| format!("--{name} needs a value"))?;
                flags.insert(name.to_string(), value.clone());
                i += 2;
            }
        }
        Ok(Self { flags })
    }

    fn get<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>> {
        match self.flags.get(name) {
            None => Ok(None),
            Some(text) => text
                .parse()
                .map(Some)
                .map_err(|_| format!("--{name}: cannot read '{text}'").into()),
        }
    }

    fn require<T: std::str::FromStr>(&self, name: &str) -> Result<T> {
        self.get(name)?
            .ok_or_else(|| format!("--{name} is required").into())
    }

    fn cycle(&self, name: &str) -> Result<Option<i64>> {
        self.flags
            .get(name)
            .map(|text| gfs::parse_cycle(text))
            .transpose()
    }

    fn data(&self) -> PathBuf {
        PathBuf::from(self.flags.get("data").map_or("data", String::as_str))
    }
}

fn run(raw: &[String]) -> Result<()> {
    let Some(command) = raw.first() else {
        println!("{USAGE}");
        return Ok(());
    };
    let args = Args::parse(&raw[1..])?;
    match command.as_str() {
        "ingest" => ingest::run(&ingest::Options {
            data: &args.data(),
            days: args.get("days")?.unwrap_or(10),
            jobs: args.get("jobs")?.unwrap_or(4),
            now: args.cycle("now")?,
        }),
        "trace" => {
            let heights: String = args.get("height")?.unwrap_or_else(|| "500".to_string());
            let heights_m = heights
                .split(',')
                .map(|h| {
                    h.parse::<f64>()
                        .map_err(|_| format!("--height: cannot read '{h}'"))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            trace::command(&trace::Options {
                data: &args.data(),
                lon: args.require("lon")?,
                lat: args.require("lat")?,
                heights_m,
                hours: args.get("hours")?.unwrap_or(240),
                time: args.cycle("time")?,
                every: args.get::<u32>("every")?.unwrap_or(6).max(1),
                json: args.flags.contains_key("json"),
            })
        }
        "synth" => synth::run(&args.data()),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command '{other}'\n\n{USAGE}").into()),
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match run(&raw) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("upwind: {e}");
            ExitCode::FAILURE
        }
    }
}
