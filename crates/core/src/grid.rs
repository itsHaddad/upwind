//! Geometry of the global wind grid and how it is cut into chunks.
//!
//! The grid is the 1° GFS grid: 360 longitudes starting at 0°E going east,
//! 181 latitudes starting at 90°N going south, on 17 pressure levels.
//!
//! It is cut three ways so that a trace downloads little more than the air
//! it passes through: into tiles, into three altitude bands, and (by the
//! file format) into days. Each tile stores 16 × 16 points and each band its
//! boundary level, so neighbouring chunks share an edge and any point can be
//! interpolated from a single chunk.
//!
//! Tiles are 15° of latitude tall. Meridians converge towards the poles, so
//! a degree of longitude there is a fraction of the distance it is at the
//! equator. Poleward of 60° tiles therefore keep every second longitude and
//! are 30° wide, and poleward of 75° every fourth and are 60° wide. That
//! keeps the distance between stored points, and the ground a tile covers,
//! roughly even, and stops polar traces from crossing a tile every hour.
//! Where a row meets a row of wider spacing, the finer row's edge holds
//! values interpolated between the coarser row's points ([`edge_stride`]),
//! so the wind is continuous across every edge.

/// Grid spacing in degrees.
pub const CELL_DEG: f64 = 1.0;
/// Number of longitudes in the global grid.
pub const NLON: usize = 360;
/// Number of latitudes in the global grid (both poles included).
pub const NLAT: usize = 181;
/// Tile edge in stored cells.
pub const TILE_CELLS: usize = 15;
/// Points along one tile edge (cells + 1, the shared edge).
pub const TILE_N: usize = TILE_CELLS + 1;
/// Rows of tiles pole to pole.
pub const TILES_Y: usize = (NLAT - 1) / TILE_CELLS;
/// Points in one horizontal plane of a tile.
pub const PLANE: usize = TILE_N * TILE_N;

/// For each row of tiles from north to south: how many grid longitudes lie
/// between stored points.
const ROW_STRIDE: [usize; TILES_Y] = [4, 2, 1, 1, 1, 1, 1, 1, 1, 1, 2, 4];

/// Tiles around the globe in a row.
const fn tiles_in_row(row: usize) -> usize {
    NLON / (TILE_CELLS * ROW_STRIDE[row])
}

/// For each row, the number of tiles in the rows north of it; the last
/// entry is the total.
const ROW_STARTS: [usize; TILES_Y + 1] = {
    let mut starts = [0; TILES_Y + 1];
    let mut row = 0;
    while row < TILES_Y {
        starts[row + 1] = starts[row] + tiles_in_row(row);
        row += 1;
    }
    starts
};

/// Number of tiles in the rows north of `row`.
const fn row_start(row: usize) -> usize {
    ROW_STARTS[row]
}

/// Total number of tiles.
pub const TILE_COUNT: usize = ROW_STARTS[TILES_Y];

/// Pressure levels in pascals, highest pressure (lowest altitude) first.
pub const LEVELS_PA: [f32; 17] = [
    100_000.0, 97_500.0, 95_000.0, 92_500.0, 90_000.0, 85_000.0, 80_000.0, 75_000.0, 70_000.0,
    65_000.0, 60_000.0, 50_000.0, 40_000.0, 30_000.0, 25_000.0, 20_000.0, 15_000.0,
];

/// The altitude bands, as inclusive ranges of indices into [`LEVELS_PA`].
/// Neighbouring bands share their boundary level: 850 hPa and 500 hPa.
pub const BANDS: [(usize, usize); 3] = [(0, 5), (5, 11), (11, 16)];
/// Number of altitude bands.
pub const BAND_COUNT: usize = BANDS.len();

/// Identifies a tile: tiles are numbered row by row from the north pole,
/// each row eastwards from 0°E.
pub type TileId = u16;
/// Identifies an altitude band: an index into [`BANDS`].
pub type BandId = u8;

/// The levels of a band, highest pressure first.
pub fn band_levels(band: BandId) -> &'static [f32] {
    let (first, last) = BANDS[band as usize];
    &LEVELS_PA[first..=last]
}

/// The band that holds a pressure. A pressure on a shared boundary level
/// belongs to the lower band.
pub fn band_of(pressure_pa: f32) -> BandId {
    BANDS
        .iter()
        .position(|(_, last)| pressure_pa >= LEVELS_PA[*last])
        .unwrap_or(BAND_COUNT - 1) as BandId
}

/// Where a geographic point falls inside the tile grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePos {
    pub tile: TileId,
    /// Index of the west column of the enclosing cell, `0..TILE_CELLS`.
    pub i: usize,
    /// Index of the north row of the enclosing cell, `0..TILE_CELLS`.
    pub j: usize,
    /// Fraction of the way east across the cell, `0..=1`.
    pub ax: f32,
    /// Fraction of the way south across the cell, `0..=1`.
    pub ay: f32,
}

/// Wraps a longitude into `[0, 360)`.
pub fn wrap_lon(lon: f64) -> f64 {
    let l = lon.rem_euclid(360.0);
    if l >= 360.0 {
        0.0
    } else {
        l
    }
}

/// Locates a point (degrees east, degrees north) in the tile grid.
pub fn locate(lon: f64, lat: f64) -> TilePos {
    let gy = (90.0 - lat.clamp(-90.0, 90.0)) / CELL_DEG;
    let (row, j, ay) = split(gy, TILES_Y);
    // Across the row, in stored cells.
    let gx = wrap_lon(lon) / (CELL_DEG * ROW_STRIDE[row] as f64);
    let (tx, i, ax) = split(gx, tiles_in_row(row));
    TilePos {
        tile: (row_start(row) + tx) as TileId,
        i,
        j,
        ax,
        ay,
    }
}

/// Splits a coordinate in cells into (tile index, cell index, fraction).
fn split(g: f64, tiles: usize) -> (usize, usize, f32) {
    let t = ((g / TILE_CELLS as f64) as usize).min(tiles - 1);
    let f = g - (t * TILE_CELLS) as f64;
    let c = (f as usize).min(TILE_CELLS - 1);
    (t, c, (f - c as f64) as f32)
}

/// The row of tiles a tile is in, and its place in that row.
fn row_of(tile: TileId) -> (usize, usize) {
    let tile = tile as usize;
    let row = (0..TILES_Y)
        .rev()
        .find(|row| row_start(*row) <= tile)
        .unwrap_or(0);
    (row, tile - row_start(row))
}

/// Grid longitudes between the points that define the wind along row `j`
/// of a tile. Inside a tile that is the tile's own spacing. Along an edge
/// shared with a row of wider spacing it is the wider one: there the finer
/// tile stores values interpolated between the coarser tile's points, so
/// that both give the same wind all along the edge.
pub fn edge_stride(tile: TileId, j: usize) -> usize {
    let (row, _) = row_of(tile);
    let neighbour = if j == 0 && row > 0 {
        ROW_STRIDE[row - 1]
    } else if j == TILE_CELLS && row + 1 < TILES_Y {
        ROW_STRIDE[row + 1]
    } else {
        ROW_STRIDE[row]
    };
    neighbour.max(ROW_STRIDE[row])
}

/// Global grid indices `(lon_index, lat_index)` of point `(i, j)` of a tile.
pub fn global_index(tile: TileId, i: usize, j: usize) -> (usize, usize) {
    let (row, tx) = row_of(tile);
    let stride = ROW_STRIDE[row];
    (
        ((tx * TILE_CELLS + i) * stride) % NLON,
        row * TILE_CELLS + j,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_have_fewer_wider_tiles_towards_the_poles() {
        let counts: Vec<usize> = (0..TILES_Y).map(tiles_in_row).collect();
        assert_eq!(counts, [6, 12, 24, 24, 24, 24, 24, 24, 24, 24, 12, 6]);
        assert_eq!(TILE_COUNT, 228);
    }

    #[test]
    fn locate_covers_the_whole_globe() {
        let p = locate(0.0, 90.0);
        assert_eq!((p.tile, p.i, p.j), (0, 0, 0));
        let p = locate(359.5, -90.0);
        assert_eq!(p.tile as usize, TILE_COUNT - 1);
        assert_eq!((p.i, p.j), (TILE_CELLS - 1, TILE_CELLS - 1));
        // In the polar rows a stored cell is four degrees wide.
        assert!((p.ax - 0.875).abs() < 1e-6 && (p.ay - 1.0).abs() < 1e-6);
        let p = locate(-0.25, 0.0);
        assert_eq!(p.tile as usize, row_start(6) + 23);
        assert!((p.ax - 0.75).abs() < 1e-6);
    }

    #[test]
    fn every_located_point_lies_between_its_cell_corners() {
        for (lon, lat) in [
            (13.19, 55.7),
            (200.3, 67.2),
            (341.9, 82.5),
            (77.7, -71.1),
            (0.0, 60.0),
        ] {
            let p = locate(lon, lat);
            let (west, north) = global_index(p.tile, p.i, p.j);
            let (east, south) = global_index(p.tile, p.i + 1, p.j + 1);
            let span = ((east + NLON - west) % NLON) as f64;
            let found_lon = west as f64 + f64::from(p.ax) * span;
            let found_lat = 90.0 - (north as f64 + f64::from(p.ay) * (south - north) as f64);
            assert!((found_lon - lon).abs() < 1e-4, "{lon},{lat}: {found_lon}");
            assert!((found_lat - lat).abs() < 1e-4, "{lon},{lat}: {found_lat}");
        }
    }

    #[test]
    fn tile_edges_wrap_to_the_global_grid() {
        // The last tile of every row ends where the first begins.
        for row in 0..TILES_Y {
            let last = (row_start(row) + tiles_in_row(row) - 1) as TileId;
            assert_eq!(global_index(last, TILE_CELLS, 0), (0, row * TILE_CELLS));
        }
        let last = (TILE_COUNT - 1) as TileId;
        assert_eq!(global_index(last, 0, TILE_CELLS), (300, NLAT - 1));
        // Neighbouring tiles share their edge column.
        assert_eq!(global_index(7, TILE_CELLS, 3), global_index(8, 0, 3));
        assert_eq!(global_index(100, TILE_CELLS, 3), global_index(101, 0, 3));
    }

    #[test]
    fn bands_share_their_boundary_levels() {
        assert_eq!(band_levels(0).last(), band_levels(1).first());
        assert_eq!(band_levels(1).last(), band_levels(2).first());
        assert_eq!(band_levels(0).first(), LEVELS_PA.first());
        assert_eq!(band_levels(2).last(), LEVELS_PA.last());
    }

    #[test]
    fn every_pressure_has_a_band_that_brackets_it() {
        assert_eq!(band_of(103_000.0), 0);
        assert_eq!(band_of(85_000.0), 0);
        assert_eq!(band_of(84_999.0), 1);
        assert_eq!(band_of(50_000.0), 1);
        assert_eq!(band_of(49_999.0), 2);
        assert_eq!(band_of(1_000.0), 2);
        for p in [99_000.0_f32, 86_000.0, 70_000.0, 51_000.0, 30_000.0] {
            let levels = band_levels(band_of(p));
            assert!(*levels.first().unwrap() >= p && p >= *levels.last().unwrap());
        }
    }
}
