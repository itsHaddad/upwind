//! Removes cached fields and published days that are no longer needed.
//!
//! Only things this tool itself writes are ever removed, recognised by
//! name: a mistyped data directory, or a stray file in the right one, is
//! left alone.

use std::fs;
use std::path::Path;

use crate::gfs::Result;
use crate::time::cycle_to_unix;
use crate::timeline::{self, Entry};

/// Whether `name` is a day directory as this tool names them:
/// `YYYYMMDD.xxxxxxxx`, possibly still `.partial`.
pub fn is_day_dir(name: &str) -> bool {
    let name = name.strip_suffix(".partial").unwrap_or(name);
    let Some((date, fingerprint)) = name.split_once('.') else {
        return false;
    };
    date.len() == 8
        && cycle_to_unix(&format!("{date}00")).is_some()
        && fingerprint.len() == 8
        && fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `cycle/file` is a cached field as this tool names them:
/// `YYYYMMDDHH/NNN.uwg`, possibly still `.partial`.
fn is_cache_file(cycle: &str, file: &str) -> bool {
    let Some((hour, extension)) = file.split_once('.') else {
        return false;
    };
    cycle_to_unix(cycle).is_some()
        && hour.len() == 3
        && hour.bytes().all(|b| b.is_ascii_digit())
        && matches!(extension, "uwg" | "partial")
}

fn name_of(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

/// Removes cached fields outside `plan` and day directories outside
/// `keep_days`. Returns how many things were removed.
pub fn prune(data: &Path, plan: &[Entry], keep_days: &[String]) -> Result<usize> {
    let mut removed = 0;
    let keep_cache: Vec<_> = plan.iter().map(|e| e.cache_path(data)).collect();
    if let Ok(cycles) = fs::read_dir(data.join("cache")) {
        for cycle in cycles {
            let cycle = cycle?.path();
            if !cycle.is_dir() {
                continue;
            }
            for file in fs::read_dir(&cycle)? {
                let file = file?.path();
                let ours = is_cache_file(name_of(&cycle), name_of(&file));
                if ours && file.is_file() && !keep_cache.contains(&file) {
                    fs::remove_file(&file)?;
                    removed += 1;
                }
            }
            if cycle_to_unix(name_of(&cycle)).is_some() && fs::read_dir(&cycle)?.next().is_none() {
                fs::remove_dir(&cycle)?;
            }
        }
    }
    if let Ok(days) = fs::read_dir(timeline::wind_dir(data)) {
        for day in days {
            let day = day?.path();
            let name = name_of(&day).to_string();
            if day.is_dir() && is_day_dir(&name) && !keep_days.contains(&name) {
                fs::remove_dir_all(&day)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_this_tool_writes_are_recognised() {
        assert!(is_day_dir("20261111.0badcafe"));
        assert!(is_day_dir("20261111.0badcafe.partial"));
        assert!(!is_day_dir("20261111"));
        assert!(!is_day_dir("20261311.0badcafe"));
        assert!(!is_day_dir("photos.2026"));
        assert!(!is_day_dir("20261111.notahash"));
        assert!(is_cache_file("2026111112", "003.uwg"));
        assert!(is_cache_file("2026111112", "003.partial"));
        assert!(!is_cache_file("2026111112", "notes.txt"));
        assert!(!is_cache_file("thumbnails", "003.uwg"));
    }

    #[test]
    fn only_stale_files_of_ours_are_removed() {
        let data = std::env::temp_dir().join(format!("upwind-prune-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let cycle = cycle_to_unix("2026111112").unwrap();
        let kept = Entry {
            cycle,
            forecast_hour: 0,
        };
        let stale = Entry {
            cycle,
            forecast_hour: 3,
        };
        let touch = |path: &Path| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        };
        touch(&kept.cache_path(&data));
        touch(&stale.cache_path(&data));
        touch(&data.join("cache/2026111112/notes.txt"));
        touch(&data.join("cache/stray-file"));
        touch(&data.join("cache/thumbnails/003.uwg"));
        touch(&data.join("wind/20261111.aaaaaaaa/0/0.uwb"));
        touch(&data.join("wind/20261110.bbbbbbbb/0/0.uwb"));
        touch(&data.join("wind/20261109.cccccccc.partial/0/0.uwb"));
        touch(&data.join("wind/holiday-photos/beach.jpg"));
        touch(&data.join("wind/manifest.json"));

        let removed = prune(&data, &[kept], &["20261111.aaaaaaaa".to_string()]).unwrap();
        assert_eq!(removed, 3, "the stale field and two stale days");
        assert!(kept.cache_path(&data).is_file());
        assert!(!stale.cache_path(&data).exists());
        assert!(data.join("wind/20261111.aaaaaaaa/0/0.uwb").is_file());
        assert!(!data.join("wind/20261110.bbbbbbbb").exists());
        assert!(!data.join("wind/20261109.cccccccc.partial").exists());
        for foreign in [
            "cache/2026111112/notes.txt",
            "cache/stray-file",
            "cache/thumbnails/003.uwg",
            "wind/holiday-photos/beach.jpg",
            "wind/manifest.json",
        ] {
            assert!(data.join(foreign).is_file(), "{foreign} must be left alone");
        }
        fs::remove_dir_all(&data).unwrap();
    }
}
