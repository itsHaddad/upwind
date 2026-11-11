//! The wind is continuous across every edge between chunks, including the
//! edges where rows of tiles store longitudes at different spacings.

use upwind_core::build::{GlobalCounts, GlobalFields};
use upwind_core::{Start, Status, Tracer, WindStore};

const TIME: i64 = 1_794_000_000;

/// A field that changes quickly from one grid point to the next, so that a
/// tile reading the wrong neighbours would be far off.
fn rough() -> GlobalCounts {
    GlobalFields::from_fn(
        TIME,
        |lon, lat, pressure| {
            let wave = (lon * 1.7).sin() * 14.0 + (lon * 0.9 + lat * 0.4).cos() * 9.0;
            (wave as f32, (wave * 0.5) as f32 - 3.0, pressure / 1.0e6)
        },
        |lon, _| (100_000.0 + 500.0 * (lon * 0.8).sin() as f32, 280.0),
    )
    .quantize()
}

fn store_of(counts: &GlobalCounts) -> WindStore {
    let mut store = WindStore::new(vec![TIME]);
    for tile in counts.tiles() {
        assert!(store.add(tile));
    }
    store
}

#[test]
fn wind_agrees_on_both_sides_of_every_edge() {
    let store = store_of(&rough());
    let sample = |lon: f64, lat: f64| store.sample(lon, lat, 90_000.0, TIME as f64).unwrap();
    let close = |a: f32, b: f32| (a - b).abs() < 0.25;
    let nudge = 1e-7;

    // Edges between rows, including the four where the spacing changes.
    for boundary in [75.0, 60.0, 45.0, 0.0, -60.0, -75.0] {
        let mut spread = 0.0_f32;
        for step in 0..720 {
            let lon = f64::from(step) * 0.5 + 0.13;
            let (north, south) = (sample(lon, boundary + nudge), sample(lon, boundary - nudge));
            assert!(
                close(north.u, south.u) && close(north.v, south.v),
                "at {lon}E {boundary}N: {north:?} against {south:?}"
            );
            assert!((north.ps - south.ps).abs() < 25.0);
            spread = spread.max(north.u.abs());
        }
        assert!(
            spread > 10.0,
            "the test field should be rough at {boundary}N"
        );
    }

    // Edges between tiles within a row, in rows of every spacing.
    for lat in [82.0, 67.0, 10.0, -67.0, -82.0] {
        for edge in (0..360).step_by(15) {
            let lon = f64::from(edge);
            let (west, east) = (sample(lon - nudge, lat), sample(lon + nudge, lat));
            assert!(
                close(west.u, east.u) && close(west.v, east.v),
                "at {lon}E {lat}N: {west:?} against {east:?}"
            );
        }
    }
}

#[test]
fn a_start_that_is_not_a_number_has_no_path_and_harms_nothing() {
    let store = store_of(&rough());
    let starts = [
        Start {
            lon: f64::NAN,
            lat: 10.0,
            height_m: 500.0,
        },
        Start {
            lon: 20.0,
            lat: 10.0,
            height_m: f64::INFINITY,
        },
        Start {
            lon: 20.0,
            lat: 10.0,
            height_m: 500.0,
        },
    ];
    let mut tracer = Tracer::new(&starts, TIME as f64, 0);
    assert_eq!(tracer.advance(&store, u32::MAX), Status::Done);
    let points = tracer.points();
    assert!(points[0].is_nan() && points[4].is_nan());
    assert!((points[8] - 20.0).abs() < 1e-6 && (points[11] - 500.0).abs() < 1.0);
    assert!(tracer.hints(&store, &[6.0]).is_empty());
}
