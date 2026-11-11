//! The integrator against flows whose trajectories are known exactly.

use upwind_core::build::GlobalFields;
use upwind_core::geo::{distance_m, EARTH_RADIUS_M};
use upwind_core::grid::locate;
use upwind_core::tracer::ROW_WIDTH;
use upwind_core::{Start, Status, Tile, Tracer, WindStore};

const T0: i64 = 1_794_000_000;
const HOUR: i64 = 3600;

type Wind = fn(f64, f64, f32) -> (f32, f32, f32);

/// Tiles for a steady flow at valid times every three hours over `hours`.
fn steady_tiles(wind: Wind, hours: i64) -> Vec<Tile> {
    (0..=hours / 3)
        .flat_map(|step| {
            let fields =
                GlobalFields::from_fn(T0 - step * 3 * HOUR, wind, |_, _| (101_000.0, 288.15));
            fields.quantize().tiles().collect::<Vec<_>>()
        })
        .collect()
}

fn store_with(tiles: &[Tile]) -> WindStore {
    let mut store = WindStore::new(tiles.iter().map(|t| t.valid_time).collect());
    for tile in tiles {
        assert!(store.add(tile.clone()));
    }
    store
}

fn trace(store: &WindStore, start: Start, hours: u32) -> Vec<f32> {
    let mut tracer = Tracer::new(&[start], T0 as f64, hours);
    assert_eq!(tracer.advance(store, u32::MAX), Status::Done);
    tracer.points().to_vec()
}

/// Longitude, latitude and pressure of the oldest point.
fn last_row(points: &[f32]) -> (f64, f64, f64) {
    let row = &points[points.len() - ROW_WIDTH..];
    (f64::from(row[0]), f64::from(row[1]), f64::from(row[2]))
}

/// The atmosphere turning like a solid body around the Earth's axis.
fn zonal(_lon: f64, lat: f64, _p: f32) -> (f32, f32, f32) {
    (40.0 * lat.to_radians().cos() as f32, 0.0, 0.0)
}

/// Solid-body rotation around the axis through 0°E on the equator, so that
/// air flows straight over both poles.
fn over_the_pole(lon: f64, lat: f64, _p: f32) -> (f32, f32, f32) {
    let (l, f) = (lon.to_radians(), lat.to_radians());
    (
        (-30.0 * f.sin() * l.cos()) as f32,
        (30.0 * l.sin()) as f32,
        0.0,
    )
}

fn sinking(_lon: f64, _lat: f64, _p: f32) -> (f32, f32, f32) {
    (0.0, 0.0, 0.1)
}

#[test]
fn zonal_flow_keeps_latitude_and_turns_at_the_right_rate() {
    let store = store_with(&steady_tiles(zonal, 48));
    let start = Start {
        lon: 10.0,
        lat: 45.0,
        height_m: 500.0,
    };
    let (lon, lat, _) = last_row(&trace(&store, start, 48));
    let turned = (40.0 * 48.0 * 3600.0 / EARTH_RADIUS_M).to_degrees();
    let expected = (10.0 - turned).rem_euclid(360.0);
    assert!((lat - 45.0).abs() < 0.02, "latitude drifted to {lat}");
    assert!(
        (lon - expected).abs() < 0.1,
        "longitude {lon}, expected {expected}"
    );
}

#[test]
fn flow_over_the_pole_arrives_on_the_far_side() {
    let store = store_with(&steady_tiles(over_the_pole, 48));
    let start = Start {
        lon: 270.0,
        lat: 80.0,
        height_m: 500.0,
    };
    let (lon, lat, _) = last_row(&trace(&store, start, 48));
    let travelled = (30.0 * 48.0 * 3600.0 / EARTH_RADIUS_M).to_degrees();
    let expected_lat = 90.0 - (travelled - 10.0);
    let error_km = distance_m(lon, lat, 90.0, expected_lat) / 1000.0;
    assert!(error_km < 40.0, "ended at {lon},{lat}, {error_km} km off");
}

#[test]
fn sinking_air_was_higher_before() {
    let store = store_with(&steady_tiles(sinking, 12));
    let start = Start {
        lon: 100.0,
        lat: -20.0,
        height_m: 3000.0,
    };
    let points = trace(&store, start, 10);
    let begin = f64::from(points[2]);
    let (_, _, end) = last_row(&points);
    assert!(
        (begin - end - 36.0).abs() < 0.05,
        "from {begin} hPa back to {end} hPa"
    );
}

#[test]
fn rising_air_stays_above_the_ground() {
    let rising: Wind = |_, _, _| (0.0, 0.0, -0.5);
    let store = store_with(&steady_tiles(rising, 12));
    let start = Start {
        lon: 0.0,
        lat: 0.0,
        height_m: 100.0,
    };
    let (_, _, end) = last_row(&trace(&store, start, 12));
    assert!((end - 1009.5).abs() < 0.01, "ended at {end} hPa");
}

#[test]
fn a_trace_that_waits_for_tiles_matches_one_that_had_them() {
    let tiles = steady_tiles(over_the_pole, 48);
    let start = Start {
        lon: 200.0,
        lat: 60.0,
        height_m: 500.0,
    };
    let eager = trace(&store_with(&tiles), start, 48);

    let mut store = WindStore::new(tiles.iter().map(|t| t.valid_time).collect());
    let mut tracer = Tracer::new(&[start], T0 as f64, 48);
    let mut rounds = 0;
    loop {
        match tracer.advance(&store, 5) {
            Status::Done => break,
            Status::Running => {}
            Status::Need(keys) => {
                rounds += 1;
                for key in keys {
                    assert!(!store.has(key), "asked again for a tile it holds");
                    let tile = tiles.iter().find(|t| (t.valid_time, t.tile, t.band) == key);
                    assert!(store.add(tile.unwrap().clone()));
                }
            }
        }
    }
    assert!(rounds > 2 && store.len() < tiles.len() / 20);
    assert_eq!(tracer.points(), eager.as_slice());
}

#[test]
fn hints_name_the_chunks_a_steady_flow_reaches_next() {
    let tiles = steady_tiles(zonal, 48);
    let start = Start {
        lon: 10.0,
        lat: 45.0,
        height_m: 500.0,
    };
    let mut store = WindStore::new(tiles.iter().map(|t| t.valid_time).collect());
    let mut tracer = Tracer::new(&[start], T0 as f64, 48);
    assert!(
        tracer.hints(&store, &[6.0]).is_empty(),
        "nothing is known before the first row"
    );

    // Run two rows, feeding the tracer what it asks for.
    let mut asked = Vec::new();
    while tracer.rows_done() < 2 {
        if let Status::Need(keys) = tracer.advance(&store, 1) {
            for key in keys {
                let tile = tiles.iter().find(|t| (t.valid_time, t.tile, t.band) == key);
                store.add(tile.unwrap().clone());
                asked.push(key);
            }
        }
    }
    // The flow turns the air around the axis at a steady rate, so 24 hours
    // back it is some thirty degrees further west, one day earlier.
    let turned = (40.0 * 24.0 * 3600.0 / EARTH_RADIUS_M).to_degrees();
    let expected = locate(10.0 - turned, 45.0).tile;
    let hints = tracer.hints(&store, &[24.0]);
    assert!(!hints.is_empty() && hints.iter().all(|key| !store.has(*key)));
    assert!(hints.iter().all(|key| key.0 <= T0 - 21 * HOUR), "{hints:?}");
    assert!(
        hints.iter().all(|key| key.1 == expected),
        "{asked:?} then {hints:?}"
    );
    assert_ne!(asked[0].1, expected);

    // A trace given its hints up front never has to ask for them.
    for key in &hints {
        let tile = tiles
            .iter()
            .find(|t| (t.valid_time, t.tile, t.band) == *key);
        assert!(store.add(tile.unwrap().clone()));
    }
    assert!(tracer.hints(&store, &[24.0]).is_empty());
}

#[test]
fn a_trace_ends_where_the_data_ends() {
    let store = store_with(&steady_tiles(zonal, 6));
    let start = Start {
        lon: 10.0,
        lat: 45.0,
        height_m: 500.0,
    };
    let points = trace(&store, start, 9);
    assert_eq!(points.len(), 10 * ROW_WIDTH);
    assert!(points[6 * ROW_WIDTH].is_finite());
    assert!(points[7 * ROW_WIDTH..].iter().all(|x| x.is_nan()));
}
