//! Traces air parcels backwards through the wind.
//!
//! Each parcel is moved with the predictor-corrector scheme HYSPLIT uses:
//! take the wind where the parcel is, step to a first-guess position, take
//! the wind there at the new time, and move with the average of the two.
//! The two winds are averaged as Earth-centred vectors, so the scheme keeps
//! its accuracy on curved paths and across the poles.
//! The step is sized so a parcel crosses at most three quarters of a grid
//! cell at a time.
//!
//! A trace can run before its wind tiles have arrived: [`Tracer::advance`]
//! stops and reports the tiles it is missing, and continues from the same
//! state once they have been added to the store.

use crate::geo::{displace, height_above_ground_m, pressure_above_ground, wind_vector};
use crate::grid::{band_of, locate};
use crate::store::{Miss, TileKey, WindStore, TOP_PRESSURE_PA};

/// Seconds between recorded positions.
pub const OUTPUT_STEP_S: f64 = 3600.0;
/// Values recorded per parcel per row: longitude, latitude, pressure in
/// hectopascals and height above the ground in metres.
pub const ROW_WIDTH: usize = 4;

const MAX_STEP_S: f64 = 3600.0;
const MIN_STEP_S: f64 = 60.0;
/// Largest part of a grid cell one step may cross.
const CELL_FRACTION: f64 = 0.75;
const CELL_M: f64 = 111_195.0;
/// Parcels are kept at least this far (pascals) above the ground.
const GROUND_CLEARANCE_PA: f64 = 50.0;
const TIME_EPSILON_S: f64 = 1e-3;

/// Where a parcel starts.
#[derive(Debug, Clone, Copy)]
pub struct Start {
    pub lon: f64,
    pub lat: f64,
    /// Height above the ground in metres.
    pub height_m: f64,
}

/// What [`Tracer::advance`] stopped on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Every row has been recorded.
    Done,
    /// The row budget ran out; call again.
    Running,
    /// These tiles must be added to the store before the trace can go on.
    Need(Vec<TileKey>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Waiting for surface pressure to turn the start height into a pressure.
    Pending {
        height_m: f64,
    },
    Active,
    /// Left the period the store covers.
    Ended,
}

#[derive(Debug, Clone, Copy)]
struct Parcel {
    lon: f64,
    lat: f64,
    pressure: f64,
    /// Surface pressure under the parcel, pascals.
    surface: f64,
    /// Temperature at the ground under the parcel, kelvin.
    surface_k: f64,
    time: f64,
    phase: Phase,
}

pub struct Tracer {
    parcels: Vec<Parcel>,
    start_time: f64,
    hours: u32,
    rows_done: u32,
    out: Vec<f32>,
}

impl Tracer {
    /// Prepares a backward trace of `hours` hours from `start_time`.
    pub fn new(starts: &[Start], start_time: f64, hours: u32) -> Self {
        let parcels = starts
            .iter()
            .map(|s| Parcel {
                lon: s.lon,
                lat: s.lat,
                pressure: 0.0,
                surface: 0.0,
                surface_k: 0.0,
                time: start_time,
                phase: Phase::Pending {
                    height_m: s.height_m,
                },
            })
            .collect::<Vec<_>>();
        let capacity = (hours as usize + 1) * parcels.len() * ROW_WIDTH;
        Self {
            parcels,
            start_time,
            hours,
            rows_done: 0,
            out: Vec::with_capacity(capacity),
        }
    }

    pub fn parcel_count(&self) -> usize {
        self.parcels.len()
    }

    /// Rows recorded so far. Row `r` is the state `r` hours before the start.
    pub fn rows_done(&self) -> u32 {
        self.rows_done
    }

    pub fn rows_total(&self) -> u32 {
        self.hours + 1
    }

    /// Recorded positions: `rows_done × parcel_count × ROW_WIDTH` values.
    /// A parcel that left the covered period is recorded as NaN.
    pub fn points(&self) -> &[f32] {
        &self.out
    }

    /// Chunks the trace will probably need soon and the store does not hold:
    /// each parcel's last hour of movement is continued in a straight line
    /// to each of `hours_ahead`. Fetching these while the trace runs saves
    /// it from stopping for them one at a time. A wrong guess costs one
    /// small download.
    pub fn hints(&self, store: &WindStore, hours_ahead: &[f64]) -> Vec<TileKey> {
        let stride = self.parcels.len() * ROW_WIDTH;
        let rows = self.rows_done as usize;
        if rows == 0 {
            return Vec::new();
        }
        let now = self.start_time - (rows - 1) as f64 * OUTPUT_STEP_S;
        let last = &self.out[(rows - 1) * stride..rows * stride];
        let before = (rows >= 2).then(|| &self.out[(rows - 2) * stride..(rows - 1) * stride]);
        let mut keys = Vec::new();
        for (index, here) in last.as_chunks::<ROW_WIDTH>().0.iter().enumerate() {
            let [lon, lat, hpa, _] = here.map(f64::from);
            if lon.is_nan() {
                continue;
            }
            let (dlon, dlat, dhpa) = before.map_or((0.0, 0.0, 0.0), |rows| {
                let [lon0, lat0, hpa0, _] = rows.as_chunks::<ROW_WIDTH>().0[index].map(f64::from);
                (
                    (lon - lon0 + 540.0).rem_euclid(360.0) - 180.0,
                    lat - lat0,
                    hpa - hpa0,
                )
            });
            for hours in hours_ahead {
                let Some((t0, t1)) = store.bracket_times(now - hours * OUTPUT_STEP_S) else {
                    continue;
                };
                let tile = locate(lon + dlon * hours, (lat + dlat * hours).clamp(-90.0, 90.0)).tile;
                let band = band_of(((hpa + dhpa * hours) * 100.0) as f32);
                keys.extend([(t0, tile, band), (t1, tile, band)]);
            }
        }
        keys.sort_unstable();
        keys.dedup();
        keys.retain(|key| !store.has(*key));
        keys
    }

    /// Runs the trace for up to `max_rows` more rows.
    pub fn advance(&mut self, store: &WindStore, max_rows: u32) -> Status {
        let top = f64::from(TOP_PRESSURE_PA);
        let mut budget = max_rows;
        while self.rows_done <= self.hours {
            if budget == 0 {
                return Status::Running;
            }
            let target = self.start_time - f64::from(self.rows_done) * OUTPUT_STEP_S;
            let mut needs: Vec<TileKey> = Vec::new();
            for parcel in &mut self.parcels {
                run_to(store, parcel, target, top, &mut needs);
            }
            if !needs.is_empty() {
                needs.sort_unstable();
                needs.dedup();
                return Status::Need(needs);
            }
            for parcel in &self.parcels {
                if parcel.phase == Phase::Active {
                    self.out.extend([
                        parcel.lon as f32,
                        parcel.lat as f32,
                        (parcel.pressure / 100.0) as f32,
                        height_above_ground_m(parcel.pressure, parcel.surface, parcel.surface_k)
                            as f32,
                    ]);
                } else {
                    self.out.extend([f32::NAN; ROW_WIDTH]);
                }
            }
            self.rows_done += 1;
            budget -= 1;
        }
        Status::Done
    }
}

/// Moves one parcel back to `target`, or as far as the loaded tiles allow.
fn run_to(store: &WindStore, parcel: &mut Parcel, target: f64, top: f64, needs: &mut Vec<TileKey>) {
    if let Phase::Pending { height_m } = parcel.phase {
        match store.sample(parcel.lon, parcel.lat, 100_000.0, parcel.time) {
            Ok(s) => {
                parcel.surface = f64::from(s.ps);
                parcel.surface_k = f64::from(s.t2);
                parcel.pressure = pressure_above_ground(parcel.surface, height_m, parcel.surface_k);
                parcel.phase = Phase::Active;
            }
            Err(miss) => return record(miss, parcel, needs),
        }
    }
    while parcel.phase == Phase::Active && parcel.time > target + TIME_EPSILON_S {
        if let Err(miss) = step(store, parcel, target, top) {
            return record(miss, parcel, needs);
        }
    }
}

fn record(miss: Miss, parcel: &mut Parcel, needs: &mut Vec<TileKey>) {
    match miss {
        Miss::Tile(key) => needs.push(key),
        Miss::OutOfTime => parcel.phase = Phase::Ended,
    }
}

/// One predictor-corrector step towards `target`. Changes nothing on a miss.
fn step(store: &WindStore, parcel: &mut Parcel, target: f64, top: f64) -> Result<(), Miss> {
    let here = store.sample(parcel.lon, parcel.lat, parcel.pressure, parcel.time)?;
    let speed = f64::from(here.u).hypot(f64::from(here.v)).max(1.0);
    let cell = CELL_M * parcel.lat.to_radians().cos().max(0.1);
    let limit = (CELL_FRACTION * cell / speed).clamp(MIN_STEP_S, MAX_STEP_S);
    let dt = -limit.min(parcel.time - target);

    let wind0 = wind_vector(parcel.lon, parcel.lat, f64::from(here.u), f64::from(here.v));
    let (lon1, lat1) = displace(parcel.lon, parcel.lat, wind0, dt);
    let p1 = confine(parcel.pressure + f64::from(here.w) * dt, here.ps, top);
    let there = store.sample(lon1, lat1, p1, parcel.time + dt)?;

    let wind1 = wind_vector(lon1, lat1, f64::from(there.u), f64::from(there.v));
    let mean = [
        0.5 * (wind0[0] + wind1[0]),
        0.5 * (wind0[1] + wind1[1]),
        0.5 * (wind0[2] + wind1[2]),
    ];
    let (lon, lat) = displace(parcel.lon, parcel.lat, mean, dt);
    let omega = 0.5 * (f64::from(here.w) + f64::from(there.w));
    parcel.pressure = confine(parcel.pressure + omega * dt, there.ps, top);
    parcel.surface = f64::from(there.ps);
    parcel.surface_k = f64::from(there.t2);
    parcel.lon = lon;
    parcel.lat = lat;
    parcel.time += dt;
    if parcel.time - target < TIME_EPSILON_S {
        parcel.time = target;
    }
    Ok(())
}

/// Keeps a pressure between the top of the data and just above the ground.
fn confine(pressure: f64, surface: f32, top: f64) -> f64 {
    let ground = f64::from(surface) - GROUND_CLEARANCE_PA;
    pressure.min(ground).max(top.min(ground))
}
