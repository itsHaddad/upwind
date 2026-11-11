//! Keeps a directory of wind up to date with the newest model run.
//!
//! Two stages: download the valid times that are not cached yet, then pack
//! every day whose set of valid times changed into wind files.
//!
//! A day that has been replaced by a newer packing stays on disk for one
//! more run. Pages that were opened before this run still hold the previous
//! manifest, and it must keep pointing at files that exist.

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::cache;
use crate::gfs::{self, Result};
use crate::pack;
use crate::prune::prune;
use crate::time::format_utc;
use crate::timeline::{self, Day, Entry, Published, TAIL_HOURS};

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

/// Runs `work` on every item with `jobs` threads; returns what failed.
fn for_each<T: Sync>(
    items: &[T],
    jobs: usize,
    work: impl Fn(&T) -> Result<()> + Sync,
) -> Vec<String> {
    let queue = Mutex::new(items.iter());
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| loop {
                let Some(item) = queue.lock().unwrap().next() else {
                    break;
                };
                if let Err(e) = work(item) {
                    failures.lock().unwrap().push(e.to_string());
                }
            });
        }
    });
    failures.into_inner().unwrap()
}

pub fn run(options: &Options) -> Result<()> {
    let started = Instant::now();
    let data = options.data;
    let agent = gfs::agent();
    let now = options.now.unwrap_or_else(unix_now);
    let latest = gfs::latest_cycle(&agent, now, TAIL_HOURS)?;
    let plan = timeline::plan(latest, options.days);
    fs::create_dir_all(timeline::wind_dir(data))?;
    let previously_published = timeline::manifest_dirs(data);

    let missing: Vec<Entry> = plan
        .iter()
        .filter(|e| !e.cache_path(data).is_file())
        .copied()
        .collect();
    eprintln!(
        "newest model run {} UTC; {} of {} times to download",
        format_utc(latest),
        missing.len(),
        plan.len()
    );
    let done = Mutex::new(0usize);
    let mut failures = for_each(&missing, options.jobs, |entry| {
        let fields = gfs::fetch_fields(&agent, entry.cycle, entry.forecast_hour)
            .map_err(|e| format!("{}: {e}", entry.name()))?;
        cache::write(&entry.cache_path(data), &fields.quantize())?;
        let mut done = done.lock().unwrap();
        *done += 1;
        eprintln!("  downloaded {}/{} {}", *done, missing.len(), entry.name());
        Ok(())
    });

    // Pack from what is cached, so one failed download costs one valid time
    // and not the whole day.
    let cached: Vec<Entry> = plan
        .iter()
        .filter(|e| e.cache_path(data).is_file())
        .copied()
        .collect();
    let days = timeline::group_days(&cached);
    let stale: Vec<&Day> = days
        .iter()
        .filter(|day| !timeline::wind_dir(data).join(day.dir()).is_dir())
        .collect();
    let bytes = Mutex::new(0u64);
    // A day in memory is about 60 MB, so pack few at a time.
    failures.extend(for_each(&stale, options.jobs.min(2), |day| {
        let fields = day
            .entries
            .iter()
            .map(|e| cache::read(&e.cache_path(data)))
            .collect::<Result<Vec<_>>>()?;
        let written = pack::write_day(data, day, &fields)?;
        *bytes.lock().unwrap() += written;
        eprintln!("  packed {} ({:.1} MB)", day.dir(), written as f64 / 1e6);
        Ok(())
    }));

    let published: Vec<Published> = days
        .iter()
        .filter(|day| timeline::wind_dir(data).join(day.dir()).is_dir())
        .map(|day| Published {
            dir: day.dir(),
            times: day.times(),
        })
        .collect();
    timeline::write_manifest(data, &published, unix_now())?;

    // Rebuilding old history with --now must not throw away the present.
    let removed = if options.now.is_none() {
        let mut keep: Vec<String> = published.iter().map(|day| day.dir.clone()).collect();
        keep.extend(previously_published);
        prune(data, &plan, &keep)?
    } else {
        0
    };
    eprintln!(
        "packed {} days ({:.1} MB), removed {removed} old items, {} days published, {:.0} s",
        stale.len(),
        *bytes.lock().unwrap() as f64 / 1e6,
        published.len(),
        started.elapsed().as_secs_f64()
    );
    if !failures.is_empty() {
        return Err(format!(
            "{} steps failed:\n  {}",
            failures.len(),
            failures.join("\n  ")
        )
        .into());
    }
    Ok(())
}
