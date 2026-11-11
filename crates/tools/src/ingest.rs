//! Keeps a directory of wind tiles up to date with the newest model run.

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::gfs::{self, Result};
use crate::time::format_utc;
use crate::timeline::{self, Entry, TAIL_HOURS};

/// Deflate effort for stored tiles.
const COMPRESSION: u8 = 8;

pub struct Options<'a> {
    pub data: &'a Path,
    pub days: u32,
    pub jobs: usize,
    /// Treat this unix time as the present (for rebuilding old history).
    pub now: Option<i64>,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Downloads one entry and writes its tiles. Returns the bytes written.
fn build_entry(agent: &ureq::Agent, data: &Path, entry: &Entry) -> Result<u64> {
    let fields = gfs::fetch_fields(agent, entry.cycle, entry.forecast_hour)?;
    let final_dir = data.join(entry.dir());
    let work_dir = data.join(format!("{}.partial", entry.dir()));
    let _ = fs::remove_dir_all(&work_dir);
    fs::create_dir_all(&work_dir)?;
    let mut written = 0;
    for tile in fields.tiles() {
        let bytes = tile.encode(COMPRESSION);
        written += bytes.len() as u64;
        fs::write(work_dir.join(format!("{}.uwt", tile.tile)), bytes)?;
    }
    let _ = fs::remove_dir_all(&final_dir);
    fs::rename(&work_dir, &final_dir)?;
    Ok(written)
}

pub fn run(options: &Options) -> Result<()> {
    let started = Instant::now();
    let agent = gfs::agent();
    let now = options.now.unwrap_or_else(unix_now);
    let latest = gfs::latest_cycle(&agent, now, TAIL_HOURS)?;
    let plan = timeline::plan(latest, options.days);
    fs::create_dir_all(options.data)?;

    let todo: Vec<Entry> = plan
        .iter()
        .filter(|e| !timeline::is_complete(options.data, e))
        .copied()
        .collect();
    eprintln!(
        "newest model run {} UTC; {} of {} times to download",
        format_utc(latest),
        todo.len(),
        plan.len()
    );

    let queue = Mutex::new(todo.iter());
    let failures = Mutex::new(Vec::new());
    let total = Mutex::new((0usize, 0u64));
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.max(1) {
            scope.spawn(|| loop {
                let Some(entry) = queue.lock().unwrap().next() else {
                    break;
                };
                match build_entry(&agent, options.data, entry) {
                    Ok(bytes) => {
                        let mut total = total.lock().unwrap();
                        total.0 += 1;
                        total.1 += bytes;
                        eprintln!(
                            "  {}/{} {} ({} kB)",
                            total.0,
                            todo.len(),
                            entry.dir(),
                            bytes / 1000
                        );
                    }
                    Err(e) => failures
                        .lock()
                        .unwrap()
                        .push(format!("{}: {e}", entry.dir())),
                }
            });
        }
    });

    let removed = timeline::prune(options.data, &plan)?;
    let available = timeline::scan(options.data)?;
    timeline::write_manifest(options.data, &available, unix_now())?;
    let (done, bytes) = *total.lock().unwrap();
    eprintln!(
        "wrote {done} times ({:.1} MB), removed {removed} old, {} in the manifest, {:.0} s",
        bytes as f64 / 1e6,
        available.len(),
        started.elapsed().as_secs_f64()
    );

    let failures = failures.into_inner().unwrap();
    if !failures.is_empty() {
        return Err(format!(
            "{} times failed:\n  {}",
            failures.len(),
            failures.join("\n  ")
        )
        .into());
    }
    Ok(())
}
