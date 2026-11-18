//! Built-in maze layouts, all designed for Glimmer.
//!
//! Every maze is 27x31 tiles, mirror-symmetric about the centre column, and
//! has exactly four flares, at least one wrap-around tunnel, no dead ends and
//! no 2x2 open areas. The tests check all of that with `Maze::validate`. The
//! character legend is in [`crate::maze`].
//!
//! On top of that, every layout follows a few design rules, checked by the
//! tests at the bottom of this file:
//! - the nest sits in a closed 7x5 wall box wrapped by a dot-free ring
//!   corridor, and the tile above the door lies on a horizontal corridor so
//!   leaving shades can turn either way;
//! - the player starts on the centre column below the nest with open floor to
//!   both sides, and the relic tile lies below the nest on the centre column;
//! - slow `t` tiles are plain tunnel corridor (never a junction), so shades only
//!   slow down when they are really heading into a tunnel.
//!
//! | # | name            | levels | door row | tunnel rows | dots | character                    |
//! |---|-----------------|--------|----------|-------------|------|------------------------------|
//! | 0 | FIREFLY GARDEN  | 1-3    | 17       | 5, 25       | 291  | corner beds, staircase lanes |
//! | 1 | CRYSTAL CAVERNS | 4-6    | 14       | 7, 22       | 293  | cross-shaped crystal loops   |
//! | 2 | MOONLIT LOOM    | 7-10   | 8        | 18          | 284  | woven lanes, high nest       |
//! | 3 | EMBER CITADEL   | 11-14  | 13       | 27          | 299  | concentric rings, sally port |
//!
//! From level 15 on the four mazes keep rotating, four levels each (see
//! [`crate::config::maze_for_level`]).

use crate::maze::MazeDef;

/// Maze 0, "Firefly Garden" (levels 1-3): a walled garden with rounded hedge
/// corners. A flower bed fills each corner (star-shaped above, cross-shaped
/// below) inside its own loop; a tunnel runs into each bed's outer tip, which
/// sits in a rounded bay of the hedge, and a flare glows on its inner tip. A
/// gated arbour crowns the top centre, diagonal staircase lanes sweep down both
/// sides between leaf-shaped beds, and the nest sits low in the garden with the
/// player starting under a small arch below it.
pub const FIREFLY_GARDEN: MazeDef = MazeDef {
    name: "FIREFLY GARDEN",
    rows: &[
        "###########################",
        "#########.........#########",
        "###...###.###.###.###...###",
        "##..#..##.#.....#.##..#..##",
        "#..###....#.###.#....###..#",
        "t.#####o###.###.###o#####.t",
        "#..###...##.###.##...###..#",
        "##..#..#.##.###.##.#..#..##",
        "###...##.##.....##.##...###",
        "###.####.##.###.##.####.###",
        "#.........................#",
        "#.###.##.####.####.##.###.#",
        "#.##..##.####.####.##..##.#",
        "#.#..###.####.####.###..#.#",
        "#...####.####.####.####...#",
        "#.####....###.###....####.#",
        "#.###..##         ##..###.#",
        "#.##..### ###-### ###..##.#",
        "#.##.#### #nnnnn# ####.##.#",
        "#........ #nnnnn# ........#",
        "#.####### #nnnnn# #######.#",
        "#...##### ####### #####...#",
        "###...###    B    ###...###",
        "###.#.###.#######.###.#.###",
        "#...#.....#######.....#...#",
        "t.#####o###.....###o#####.t",
        "#...#....##.###.##....#...#",
        "###.#.##.....P.....##.#.###",
        "###...###.###.###.###...###",
        "#########.........#########",
        "###########################",
    ],
};

/// Maze 1, "Crystal Caverns" (levels 4-6): two cross-shaped crystals hang in
/// the upper caverns and two T-shaped shards rise from the cavern floor, each
/// wrapped by its own winding loop. Two tunnels: a high one that opens straight
/// into the upper crystal loops and a low one on the player's starting lane.
/// The nest sits a little below the centre, fed by a straight central shaft.
pub const CRYSTAL_CAVERNS: MazeDef = MazeDef {
    name: "CRYSTAL CAVERNS",
    rows: &[
        "###########################",
        "#.........................#",
        "#.#####.#####.#####.#####.#",
        "#.........###.###.........#",
        "#o###.###.###.###.###.###o#",
        "#.....###...#.#...###.....#",
        "###.#######.#.#.#######.###",
        "ttt.#######.....#######.ttt",
        "###.#######.#.#.#######.###",
        "#.....###...#.#...###.....#",
        "#.###.###.###.###.###.###.#",
        "#.###.....###.###.....###.#",
        "#.#######.###.###.#######.#",
        "#........         ........#",
        "#.###.### ###-### ###.###.#",
        "#.###.### #nnnnn# ###.###.#",
        "#.###.... #nnnnn# ....###.#",
        "#.###.### #nnnnn# ###.###.#",
        "#.###.### ####### ###.###.#",
        "#.....###    B    ###.....#",
        "#####.###.#######.###.#####",
        "#####.###.#######.###.#####",
        "tttt.........P.........tttt",
        "#######.#####.#####.#######",
        "#.........###.###.........#",
        "#.###.###.###.###.###.###.#",
        "#o#...###...#.#...###...#o#",
        "#.#.#######.#.#.#######.#.#",
        "#.#.#######.#.#.#######.#.#",
        "#.........................#",
        "###########################",
    ],
};

/// Maze 2, "Moonlit Loom" (levels 7-10): long full-width lanes woven
/// together by thin weft walls with staggered gaps, heddle-shaped T blocks on
/// the centre line and brick-bonded side pockets. The nest hangs high in the
/// maze; a single long "shuttle" tunnel crosses the lower half.
pub const MOONLIT_LOOM: MazeDef = MazeDef {
    name: "MOONLIT LOOM",
    rows: &[
        "###########################",
        "#o.......................o#",
        "#.##.####.#######.####.##.#",
        "#.........#######.........#",
        "#.#####.###########.#####.#",
        "#.#####.###########.#####.#",
        "#.#####.###########.#####.#",
        "#........         ........#",
        "#.###.### ###-### ###.###.#",
        "#.###.### #nnnnn# ###.###.#",
        "#.....### #nnnnn# ###.....#",
        "###.##### #nnnnn# #####.###",
        "###.##### ####### #####.###",
        "###......    B    ......###",
        "###.#######.###.#######.###",
        "###.#######.###.#######.###",
        "###.........###.........###",
        "#######.###########.#######",
        "tttt...................tttt",
        "####.#####.#####.#####.####",
        "####.#####.#####.#####.####",
        "#............P............#",
        "#.#####.###########.#####.#",
        "#...........###...........#",
        "###.#######.###.#######.###",
        "###.#######.###.#######.###",
        "###.#######.###.#######.###",
        "#o.......................o#",
        "#.##.####.#######.####.##.#",
        "#.........................#",
        "###########################",
    ],
};

/// Maze 3, "Ember Citadel" (levels 11-14): the nest is the keep at the very
/// centre of three concentric curtain walls whose gates are staggered, so every
/// approach winds around a ring. Gatehouses guard the top and bottom of the
/// outer wall and a short sally-port tunnel runs along the bottom.
pub const EMBER_CITADEL: MazeDef = MazeDef {
    name: "EMBER CITADEL",
    rows: &[
        "###########################",
        "#...........###...........#",
        "#.###.#####.###.#####.###.#",
        "#o###.#####.....#####.###o#",
        "#.###.#######.#######.###.#",
        "#.........................#",
        "#.#.#####.#######.#####.#.#",
        "#.#.#####.#######.#####.#.#",
        "#.#.#####.#######.#####.#.#",
        "#.#.##...............##.#.#",
        "#...##.######.######.##...#",
        "#.#.##.######.######.##.#.#",
        "#.#....##         ##....#.#",
        "#.#.##.## ###-### ##.##.#.#",
        "#.#.##.## #nnnnn# ##.##.#.#",
        "#.#.##... #nnnnn# ...##.#.#",
        "#.#.##.## #nnnnn# ##.##.#.#",
        "#.#.##.## ####### ##.##.#.#",
        "#.#....##    B    ##....#.#",
        "#.#.##.######.######.##.#.#",
        "#...##.######.######.##...#",
        "#.#.##.......P.......##.#.#",
        "#.#.#####.#######.#####.#.#",
        "#o#.#####.#######.#####.#o#",
        "#.#.#####.#######.#####.#.#",
        "#.........................#",
        "#.###.#######.#######.###.#",
        "t.....#####.....#####.....t",
        "#.###.#####.###.#####.###.#",
        "#...........###...........#",
        "###########################",
    ],
};

/// All built-in mazes in rotation order (see `config::maze_for_level`).
pub const ALL: &[MazeDef] = &[FIREFLY_GARDEN, CRYSTAL_CAVERNS, MOONLIT_LOOM, EMBER_CITADEL];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{maze_for_level, MAZE_H, MAZE_W};
    use crate::geom::{Dir, Tile};
    use crate::maze::{Cell, Maze};

    /// Parse built-in maze `i`, failing the test with the parser's message.
    fn parse(i: usize) -> Maze {
        let def = &ALL[i];
        Maze::parse(def.name, def.rows, i)
            .unwrap_or_else(|e| panic!("maze {i} ({}) does not parse: {e}", def.name))
    }

    fn all_mazes() -> Vec<Maze> {
        (0..ALL.len()).map(parse).collect()
    }

    #[test]
    fn rotation_covers_every_level() {
        assert_eq!(ALL.len(), 4);
        assert_eq!(ALL[0].name, FIREFLY_GARDEN.name);
        assert_eq!(Maze::builtin_count(), ALL.len());
        for level in 0..200 {
            assert!(maze_for_level(level) < ALL.len(), "level {level}");
        }
        for (i, def) in ALL.iter().enumerate() {
            let m = Maze::builtin(i);
            assert_eq!(m.index, i);
            assert_eq!(m.name, def.name);
            assert!(
                (1..200).any(|level| maze_for_level(level) == i),
                "maze {i} ({}) is never played",
                def.name
            );
        }
    }

    #[test]
    fn names_are_unique_and_uppercase() {
        for (i, def) in ALL.iter().enumerate() {
            let name = def.name;
            assert!(!name.is_empty(), "maze {i} has no name");
            assert!(
                name.chars().all(|c| c.is_ascii_uppercase() || c == ' '),
                "maze {i} name {name:?} must be uppercase letters and spaces"
            );
            assert_eq!(name.trim(), name, "maze {i} name has stray spaces");
            assert!(!name.contains("  "), "maze {i} name has double spaces");
            for other in &ALL[i + 1..] {
                assert_ne!(name, other.name, "duplicate maze name");
            }
        }
    }

    #[test]
    fn layouts_are_pairwise_different() {
        for (i, a) in ALL.iter().enumerate() {
            for (j, b) in ALL.iter().enumerate().skip(i + 1) {
                assert_ne!(a.rows, b.rows, "mazes {i} and {j} share a layout");
                // Not just different: at least a quarter of all tiles differ.
                let differing: usize = a
                    .rows
                    .iter()
                    .zip(b.rows)
                    .map(|(ra, rb)| ra.chars().zip(rb.chars()).filter(|(x, y)| x != y).count())
                    .sum();
                let total = (MAZE_W * MAZE_H) as usize;
                assert!(
                    differing * 4 >= total,
                    "mazes {i} and {j} differ in only {differing} of {total} tiles"
                );
            }
        }
        // Each maze also has its own structural signature (nest row + tunnels).
        let mazes = all_mazes();
        for (i, a) in mazes.iter().enumerate() {
            for (j, b) in mazes.iter().enumerate().skip(i + 1) {
                assert!(
                    a.door.y != b.door.y || a.tunnel_rows != b.tunnel_rows,
                    "mazes {i} and {j} share nest row and tunnel rows"
                );
            }
        }
    }

    #[test]
    fn every_maze_parses_and_validates() {
        for (i, m) in all_mazes().iter().enumerate() {
            assert_eq!((m.w, m.h), (MAZE_W, MAZE_H), "maze {i} size");
            assert_eq!(m.index, i);
            let errs = m.validate();
            assert!(
                errs.is_empty(),
                "maze {i} ({}) invalid:\n{}",
                m.name,
                errs.join("\n")
            );
        }
    }

    #[test]
    fn dot_counts_are_in_range() {
        for (i, m) in all_mazes().iter().enumerate() {
            assert!(
                (180..=320).contains(&m.total_dots),
                "maze {i} ({}) has {} dots",
                m.name,
                m.total_dots
            );
            assert_eq!(m.dots_left(), m.total_dots);
            let flares = m.dots().filter(|&(_, c)| c == Cell::Flare).count();
            assert_eq!(flares, 4, "maze {i} flares");
        }
    }

    #[test]
    fn tunnels_exist_and_nest_exit_lets_shades_turn() {
        for (i, m) in all_mazes().iter().enumerate() {
            assert!(
                !m.tunnel_rows.is_empty(),
                "maze {i} ({}) has no tunnel",
                m.name
            );
            for d in [Dir::Left, Dir::Right] {
                assert!(
                    m.is_walkable(m.nest_exit.step(d)),
                    "maze {i} ({}): nest exit {:?} is not on a horizontal corridor",
                    m.name,
                    m.nest_exit
                );
            }
        }
    }

    #[test]
    fn nest_is_boxed_in_and_ringed_by_a_corridor() {
        for (i, m) in all_mazes().iter().enumerate() {
            let d = m.door;
            // 7 x 5 wall box (door excepted) around the 5 x 3 interior.
            for y in d.y..=d.y + 4 {
                for x in d.x - 3..=d.x + 3 {
                    let t = Tile::new(x, y);
                    let interior =
                        (d.y + 1..=d.y + 3).contains(&y) && (d.x - 2..=d.x + 2).contains(&x);
                    let expected = if t == d {
                        Cell::Door
                    } else if interior {
                        Cell::Nest
                    } else {
                        Cell::Wall
                    };
                    assert_eq!(m.cell(t), expected, "maze {i} ({}) nest tile {t:?}", m.name);
                }
            }
            // Dot-free ring corridor one tile outside the box.
            let ring = (d.x - 4..=d.x + 4)
                .flat_map(|x| [Tile::new(x, d.y - 1), Tile::new(x, d.y + 5)])
                .chain(
                    (d.y - 1..=d.y + 5)
                        .flat_map(|y| [Tile::new(d.x - 4, y), Tile::new(d.x + 4, y)]),
                );
            for t in ring {
                assert_eq!(
                    m.cell(t),
                    Cell::Open,
                    "maze {i} ({}) ring tile {t:?}",
                    m.name
                );
            }
        }
    }

    #[test]
    fn player_and_relic_tiles_sit_below_the_nest() {
        for (i, m) in all_mazes().iter().enumerate() {
            let p = m.player_start;
            assert_eq!(p.x, m.w / 2, "maze {i} player column");
            assert!(
                p.y > m.door.y + 4,
                "maze {i} player must start below the nest"
            );
            for d in [Dir::Left, Dir::Right] {
                assert!(
                    m.is_walkable(p.step(d)),
                    "maze {i} player start needs floor {d:?}"
                );
            }
            let b = m.bonus_tile;
            assert_eq!(b.x, m.w / 2, "maze {i} relic column");
            assert!(
                b.y > m.door.y + 4,
                "maze {i} relic must appear below the nest"
            );
            assert_eq!(m.dot_at(b), None);
        }
    }

    #[test]
    fn slow_tiles_are_plain_tunnel_corridor() {
        for (i, m) in all_mazes().iter().enumerate() {
            for y in 0..m.h {
                for x in 0..m.w {
                    let t = Tile::new(x, y);
                    if !m.is_slow(t) {
                        continue;
                    }
                    assert!(
                        m.is_tunnel_row(y),
                        "maze {i}: slow tile {t:?} off a tunnel row"
                    );
                    let exits: Vec<Dir> = m.exits(t).collect();
                    assert_eq!(
                        exits,
                        [Dir::Left, Dir::Right],
                        "maze {i}: slow tile {t:?} must be a straight horizontal corridor"
                    );
                }
            }
            for &y in &m.tunnel_rows {
                assert!(
                    m.is_slow(Tile::new(0, y)),
                    "maze {i}: tunnel mouth on row {y} is not slow"
                );
                assert!(
                    m.is_slow(Tile::new(m.w - 1, y)),
                    "maze {i}: tunnel mouth on row {y} is not slow"
                );
            }
        }
    }

    #[test]
    fn landmark_rows_are_unique_per_maze() {
        // Stronger than the signature check above: no two mazes put their
        // nest door on the same row or share any tunnel row.
        let mazes = all_mazes();
        for (i, a) in mazes.iter().enumerate() {
            for (j, b) in mazes.iter().enumerate().skip(i + 1) {
                assert_ne!(a.door.y, b.door.y, "mazes {i} and {j} share the door row");
                for y in &a.tunnel_rows {
                    assert!(
                        !b.tunnel_rows.contains(y),
                        "mazes {i} and {j} share tunnel row {y}"
                    );
                }
            }
        }
    }

    /// Landmarks of the classic arcade board of this genre: the door row, side
    /// tunnel row, player start row, bonus row and the corner power-dot spots
    /// (next to the side walls on rows 3 and 22/23).
    const CLASSIC_DOOR_ROW: i32 = 12;
    const CLASSIC_TUNNEL_ROW: i32 = 14;
    const CLASSIC_START_ROW: i32 = 23;
    const CLASSIC_BONUS_ROW: i32 = 17;
    const CLASSIC_CORNER_ROWS: [i32; 3] = [3, 22, 23];

    /// Which of those landmarks maze `m` shares.
    fn classic_landmarks(m: &Maze) -> Vec<&'static str> {
        let mut shared = Vec::new();
        if m.door.y == CLASSIC_DOOR_ROW {
            shared.push("door row");
        }
        if m.tunnel_rows.contains(&CLASSIC_TUNNEL_ROW) {
            shared.push("tunnel row");
        }
        if m.player_start.y == CLASSIC_START_ROW {
            shared.push("start row");
        }
        if m.bonus_tile.y == CLASSIC_BONUS_ROW {
            shared.push("bonus row");
        }
        let corner_flare = m.dots().any(|(t, c)| {
            c == Cell::Flare && (t.x == 1 || t.x == m.w - 2) && CLASSIC_CORNER_ROWS.contains(&t.y)
        });
        if corner_flare {
            shared.push("corner flares");
        }
        shared
    }

    #[test]
    fn no_maze_reuses_the_classic_arcade_skeleton() {
        for (i, m) in all_mazes().iter().enumerate() {
            let shared = classic_landmarks(m);
            assert!(
                shared.len() <= 1,
                "maze {i} ({}) shares classic landmarks {shared:?}",
                m.name
            );
        }
        // The first maze every player sees shares none of them and keeps its
        // whole nest box clear of the classic nest band (rows 12-16).
        let m = parse(0);
        assert_eq!(classic_landmarks(&m), Vec::<&str>::new());
        assert!(m.door.y > 16, "maze 0 nest overlaps the classic nest band");
    }

    #[test]
    fn firefly_garden_layout() {
        let m = parse(0);
        assert_eq!(m.name, "FIREFLY GARDEN");
        // Nest in the lower half; relic on the ring row right below the nest.
        assert!(m.door.y > m.h / 2, "door row {}", m.door.y);
        assert_eq!(m.bonus_tile, Tile::new(m.w / 2, m.door.y + 5));
        // One tunnel in the top third and one in the bottom third.
        assert_eq!(m.tunnel_rows.len(), 2);
        assert!(m.tunnel_rows[0] < m.h / 3, "{:?}", m.tunnel_rows);
        assert!(m.tunnel_rows[1] >= m.h * 2 / 3, "{:?}", m.tunnel_rows);
        // Flares glow on the inner tips of the corner beds: on inner lanes,
        // never next to the side walls, each facing a tunnel mouth across
        // its bed.
        let flares: Vec<Tile> = m
            .dots()
            .filter(|&(_, c)| c == Cell::Flare)
            .map(|(t, _)| t)
            .collect();
        assert_eq!(flares.len(), 4);
        for t in flares {
            assert!(t.x > 3 && t.x < m.w - 4, "flare {t:?} hugs a side wall");
            assert!(m.is_tunnel_row(t.y), "flare {t:?} is off the tunnel rows");
        }
        // The player starts on a lane with room to move both ways and a
        // junction within a few steps on each side.
        let p = m.player_start;
        for d in [Dir::Left, Dir::Right] {
            let junction = (1..=4).any(|n| m.exits(p.ahead(d, n)).count() >= 3);
            assert!(junction, "no junction near the start going {d:?}");
        }
    }

    /// Longest run of corridor tiles (exactly two exits) between junctions,
    /// following tunnels around the board.
    fn longest_corridor(m: &Maze) -> usize {
        let is_junction = |t: Tile| m.exits(t).count() >= 3;
        let mut longest = 0;
        for t in m.walkable_tiles().into_iter().filter(|&t| is_junction(t)) {
            for first in m.exits(t) {
                let mut dir = first;
                let mut cur = m.wrap_tile(t.step(dir));
                let mut len = 0;
                while !is_junction(cur) {
                    len += 1;
                    assert!(len < 200, "corridor from {t:?} never meets a junction");
                    dir = m
                        .exits(cur)
                        .find(|&e| e != dir.opposite())
                        .expect("validated mazes have no dead ends");
                    cur = m.wrap_tile(cur.step(dir));
                }
                longest = longest.max(len);
            }
        }
        longest
    }

    #[test]
    fn corridors_have_no_long_single_file_stretches() {
        for (i, m) in all_mazes().iter().enumerate() {
            let len = longest_corridor(m);
            // Every maze has some plain corridor, so the measure is non-trivial.
            assert!(len >= 4, "maze {i} ({}): measure broken ({len})", m.name);
            assert!(
                len <= 12,
                "maze {i} ({}) has a {len}-tile single-file corridor",
                m.name
            );
        }
    }
}
