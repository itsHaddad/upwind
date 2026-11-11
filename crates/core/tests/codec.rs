//! The wind file format: what goes in comes out, and damage is refused.

use upwind_core::codec::{
    decode, encode, quantize, quantize_ps, quantize_t, read_header, DecodeError, OMEGA_SCALE,
    WIND_SCALE,
};
use upwind_core::grid::{band_levels, PLANE};
use upwind_core::Tile;

fn sample_tile(valid_time: i64) -> Tile {
    let levels_pa = band_levels(1).to_vec();
    let n = levels_pa.len() * PLANE;
    let seed = (valid_time % 1000) as f32;
    let wave = |k: usize, f: f32| quantize(40.0 * ((k as f32) * f + seed).sin(), WIND_SCALE);
    Tile {
        valid_time,
        tile: 217,
        band: 1,
        levels_pa,
        wind_scale: WIND_SCALE,
        omega_scale: OMEGA_SCALE,
        u: (0..n).map(|k| wave(k, 0.013)).collect(),
        v: (0..n).map(|k| wave(k, 0.021)).collect(),
        w: (0..n)
            .map(|k| if k % 97 == 0 { i16::MIN } else { i16::MAX })
            .collect(),
        ps: (0..PLANE)
            .map(|k| quantize_ps(98_000.0 + k as f32))
            .collect(),
        t2: (0..PLANE)
            .map(|k| quantize_t(250.0 + (k % 60) as f32))
            .collect(),
    }
}

fn sample_day() -> Vec<Tile> {
    (0..8)
        .map(|step| sample_tile(1_794_000_000 + step * 10_800))
        .collect()
}

#[test]
fn round_trips_exactly() {
    let day = sample_day();
    let bytes = encode(&day, 6);
    assert_eq!(decode(&bytes).unwrap(), day);
    let header = read_header(&bytes).unwrap();
    assert_eq!((header.tile, header.band), (217, 1));
    assert_eq!(header.valid_times.len(), 8);
    assert_eq!(header.valid_times[1] - header.valid_times[0], 10_800);
}

#[test]
fn smooth_fields_compress() {
    let mut day = sample_day();
    for tile in &mut day {
        tile.w.fill(0);
    }
    let counts = 3 * day[0].levels() * PLANE + 2 * PLANE;
    let raw = day.len() * counts * 2;
    assert!(encode(&day, 6).len() * 3 < raw);
}

#[test]
fn rejects_damaged_input() {
    let bytes = encode(&sample_day(), 6);
    assert_eq!(decode(&bytes[..10]), Err(DecodeError::TooShort));
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert_eq!(decode(&bad), Err(DecodeError::BadMagic));
    let mut bad = bytes.clone();
    bad[6] = 9;
    assert_eq!(decode(&bad), Err(DecodeError::BadShape));
    assert_eq!(
        decode(&bytes[..bytes.len() - 20]),
        Err(DecodeError::BadPayload)
    );
}

#[test]
#[should_panic(expected = "share tile, band")]
fn refuses_to_mix_tiles_in_one_file() {
    let mut day = sample_day();
    day[1].tile = 3;
    encode(&day, 6);
}
