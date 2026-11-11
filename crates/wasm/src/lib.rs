//! The trajectory engine behind a plain C interface, for WebAssembly.
//!
//! The page owns one engine. It hands over tile files as it downloads them,
//! starts a trace, and calls [`uw_advance`] until the trace is done; when the
//! engine reports that it needs tiles, the page fetches those and calls
//! again. Buffers are passed as pointer and length into the module's memory,
//! allocated with [`uw_alloc`].

use std::cell::RefCell;

use upwind_core::{Start, Status, Tile, Tracer, WindStore};

/// [`uw_advance`] results.
const DONE: i32 = 0;
const RUNNING: i32 = 1;
const NEED: i32 = 2;
const NOT_STARTED: i32 = -1;

struct Engine {
    store: WindStore,
    tracer: Option<Tracer>,
    /// Tiles the last advance asked for, as `(valid_time, tile)` pairs.
    needs: Vec<f64>,
}

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine {
        store: WindStore::new(Vec::new()),
        tracer: None,
        needs: Vec::new(),
    });
}

fn with<T>(f: impl FnOnce(&mut Engine) -> T) -> T {
    ENGINE.with(|engine| f(&mut engine.borrow_mut()))
}

/// Allocates `len` bytes inside the module for the page to fill.
#[no_mangle]
pub extern "C" fn uw_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u64>::with_capacity(len.div_ceil(8).max(1));
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer.cast()
}

/// Frees a buffer from [`uw_alloc`].
///
/// # Safety
/// `pointer` and `len` must be exactly what a call to `uw_alloc` used.
#[no_mangle]
pub unsafe extern "C" fn uw_free(pointer: *mut u8, len: usize) {
    let capacity = len.div_ceil(8).max(1);
    drop(unsafe { Vec::<u64>::from_raw_parts(pointer.cast(), 0, capacity) });
}

/// Empties the engine and sets the timeline: `count` valid times in unix
/// seconds.
///
/// # Safety
/// `times` must point to `count` readable f64 values.
#[no_mangle]
pub unsafe extern "C" fn uw_reset(times: *const f64, count: usize) {
    let times = unsafe { std::slice::from_raw_parts(times, count) };
    with(|engine| {
        engine.store = WindStore::new(times.iter().map(|t| *t as i64).collect());
        engine.tracer = None;
        engine.needs.clear();
    });
}

/// Adds a downloaded tile file. Returns 1 when added, 0 when its time is
/// not on the timeline, -1 when the file is damaged.
///
/// # Safety
/// `bytes` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn uw_add_tile(bytes: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    match Tile::decode(bytes) {
        Ok(tile) => with(|engine| i32::from(engine.store.add(tile))),
        Err(_) => -1,
    }
}

/// Starts a backward trace of `hours` hours from `start_time` (unix
/// seconds) for `count` parcels given as `(lon, lat, height_m)` triples.
///
/// # Safety
/// `starts` must point to `3 × count` readable f64 values.
#[no_mangle]
pub unsafe extern "C" fn uw_start(starts: *const f64, count: usize, start_time: f64, hours: u32) {
    let values = unsafe { std::slice::from_raw_parts(starts, count * 3) };
    let starts: Vec<Start> = values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|s| Start {
            lon: s[0],
            lat: s[1],
            height_m: s[2],
        })
        .collect();
    with(|engine| {
        engine.tracer = Some(Tracer::new(&starts, start_time, hours));
        engine.needs.clear();
    });
}

/// Runs the trace for up to `max_rows` more hourly rows. Returns 0 when the
/// trace is complete, 1 when the budget ran out, 2 when tiles are needed
/// (see [`uw_needs`]), -1 when no trace was started.
#[no_mangle]
pub extern "C" fn uw_advance(max_rows: u32) -> i32 {
    with(|engine| {
        let Some(tracer) = engine.tracer.as_mut() else {
            return NOT_STARTED;
        };
        engine.needs.clear();
        match tracer.advance(&engine.store, max_rows) {
            Status::Done => DONE,
            Status::Running => RUNNING,
            Status::Need(keys) => {
                for (time, tile) in keys {
                    engine.needs.extend([time as f64, f64::from(tile)]);
                }
                NEED
            }
        }
    })
}

/// Pointer to the needed tiles: [`uw_needs_len`] f64 values, in
/// `(valid_time, tile)` pairs. Valid until the next call into the engine.
#[no_mangle]
pub extern "C" fn uw_needs() -> *const f64 {
    with(|engine| engine.needs.as_ptr())
}

#[no_mangle]
pub extern "C" fn uw_needs_len() -> usize {
    with(|engine| engine.needs.len())
}

/// Hourly rows recorded so far.
#[no_mangle]
pub extern "C" fn uw_rows() -> u32 {
    with(|engine| engine.tracer.as_ref().map_or(0, Tracer::rows_done))
}

/// Pointer to the recorded positions: [`uw_points_len`] f32 values, three
/// per parcel per row (longitude, latitude, pressure in hectopascals).
/// Valid until the next call into the engine.
#[no_mangle]
pub extern "C" fn uw_points() -> *const f32 {
    with(|engine| {
        engine
            .tracer
            .as_ref()
            .map_or(std::ptr::null(), |t| t.points().as_ptr())
    })
}

#[no_mangle]
pub extern "C" fn uw_points_len() -> usize {
    with(|engine| engine.tracer.as_ref().map_or(0, |t| t.points().len()))
}
