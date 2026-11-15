//! A `.blueprint` of the MineColonies corpus, converted to a vanilla structure
//! template by `bp2nbt` (the `port-blueprint` crate), loaded by the
//! `StructureTemplateManager` and placed by `place_template_entry`: every cell
//! is read back and compared to the blueprint, then the same template is placed
//! again rotated a quarter turn and compared to the turned states.
//!
//! What it follows, by name:
//!
//! * `StructureTemplateManager.get` / `StructureTemplate.load`:
//!   `Game::template_entry` (`structure_blocks.rs:150`) reading
//!   `<world_root>/generated/<ns>/structures/<path>.nbt` (`place.rs:60-69`);
//! * `StructureTemplate.placeInWorld`: `place_template_entry`
//!   (`structure_blocks.rs:384`) → `Behaviours::place_template`
//!   (`crates/horizon-world/src/level/bonemeal.rs:162`) → `place_in_world`
//!   (`crates/horizon-world/src/worldgen/structure/processor.rs:1491`);
//! * `StructurePlaceSettings` with `knownShape` (the command's `strict`):
//!   `load_settings` (`structure_blocks.rs:663`), so no neighbour shape update
//!   rewrites the stairs the template carries;
//! * `StructureTemplate.transform(pos, mirror, rotation, pivot)` for the cell a
//!   template block lands on (`processor.rs:1408`), and `BlockState.rotate`
//!   (the `structure-blocks.json` table, `processor.rs:1543`) for its state.
//!
//! The blueprint is `minecolonies/lostcity/infrastructure/roads/simple/
//! simple_bridge_cross_pillars.blueprint` (5×4×4, 80 cells, 7 palette entries,
//! 6 of them with properties): four `oak_stairs` facing the four directions with
//! `half=top`, an `oak_slab[type=top]` and `oak_planks`. It is one of the 48
//! blueprints of the corpus whose palette is entirely `minecraft:` — the only
//! ones this engine can place today, and none of those carries a block entity or
//! an entity.
//!
//! Two inputs live outside this repository, and the test stops with a message
//! without them:
//!
//! ```sh
//! HORIZON_BP2NBT=/home/zaccou/opencode-server/workspace/port-blueprint/target/release/bp2nbt \
//! HORIZON_MINECOLONIES_BLUEPRINTS=/home/zaccou/opencode-server/workspace/minecolonies-src/src/main/resources/blueprints \
//! HORIZON_TEST_RESOURCES=... HORIZON_HOME=... \
//!   cargo test -p horizon-client --lib game::blueprint_nbt_tests -- --ignored --nocapture
//! ```
use super::native_entity_refs::tests::with_game;
use crate::game::{
    Game,
    structure_blocks::{load_settings, template_size},
};
use horizon_client::chunk::Chunk;
use horizon_core::{
    nbt,
    nbt::{Limits, Tag},
    position::BlockPos,
};
use horizon_world::{
    chunk::BlockState,
    compression::Compression,
    worldgen::structure::{Mirror, Rotation},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The blueprint this test converts: the corpus path of the file inside
/// `HORIZON_MINECOLONIES_BLUEPRINTS`.
const BLUEPRINT: &str =
    "minecolonies/lostcity/infrastructure/roads/simple/simple_bridge_cross_pillars.blueprint";
/// The name the template is saved and loaded under (`test:` is not a namespace
/// the game ships, so a `generated/test/structures/` folder is unambiguous).
const TEMPLATE: &str = "test:blueprint_nbt";
/// The placement origin of the unrotated copy, and of the rotated one: the same
/// chunk, four blocks apart vertically, so neither box touches the other.
const ORIGIN: [i32; 3] = [8, 64, 8];
const ROTATED_ORIGIN: [i32; 3] = [8, 68, 8];
/// The stone both boxes are filled with before a placement, so a cell the
/// blueprint holds as air has to be written to read back as air.
const STONE_BOX: ([i32; 3], [i32; 3]) = ([4, 63, 7], [13, 72, 13]);

/// One cell of the blueprint: its position in the template's own frame and the
/// complete state Structurize wrote there.
#[derive(Clone, Debug)]
struct Cell {
    pos: [i32; 3],
    name: String,
    properties: BTreeMap<String, String>,
}

/// `bp2nbt` and the blueprint corpus, or `None` with the reason printed: both
/// live outside this repository, so a checkout without them cannot run the test.
fn converter_inputs() -> Option<(PathBuf, PathBuf)> {
    let (Some(bp2nbt), Some(corpus)) = (
        std::env::var_os("HORIZON_BP2NBT"),
        std::env::var_os("HORIZON_MINECOLONIES_BLUEPRINTS"),
    ) else {
        println!(
            "skipping: this test converts a real blueprint with the port-blueprint crate's \
             binary, which is not in this repository. Set HORIZON_BP2NBT (the built `bp2nbt`) \
             and HORIZON_MINECOLONIES_BLUEPRINTS (the MineColonies `blueprints/` folder) to run it."
        );
        return None;
    };
    let (bp2nbt, corpus) = (PathBuf::from(bp2nbt), PathBuf::from(corpus));
    assert!(
        bp2nbt.is_file(),
        "HORIZON_BP2NBT does not name a file: {}",
        bp2nbt.display()
    );
    assert!(
        corpus.is_dir(),
        "HORIZON_MINECOLONIES_BLUEPRINTS does not name a folder: {}",
        corpus.display()
    );
    Some((bp2nbt, corpus))
}

/// `StructureTemplate`'s size tags: `size_x`/`size_y`/`size_z` are shorts in a
/// blueprint (`BlueprintUtil.java:212-214`), ints in a template.
fn blueprint_size(tag: &Tag) -> [i32; 3] {
    ["size_x", "size_y", "size_z"].map(|key| match tag.get(key) {
        Some(Tag::Short(v)) => i32::from(*v),
        Some(Tag::Int(v)) => *v,
        other => panic!("blueprint {key}: expected a short, found {other:?}"),
    })
}

/// The blueprint as the cells it holds, read the way Structurize writes them:
/// a `palette` of `{Name, Properties}` and a `blocks` int array holding two
/// 16-bit palette indices per int (the even cell's in the high half), in
/// y → z → x order (`BlueprintUtil.java:649-674`, `Blueprint.java:264-271`).
///
/// This is the oracle, deliberately not the converter's own reader: a
/// conversion that dropped a cell or a property has to fail here.
fn read_blueprint(path: &Path) -> ([i32; 3], Vec<Cell>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let raw = Compression::Gzip
        .decode(&bytes, 1 << 28)
        .unwrap_or_else(|e| panic!("{}: not one gzip member: {e}", path.display()));
    let (named, _) = nbt::read_named(&raw, Limits::templates())
        .unwrap_or_else(|e| panic!("{}: not an NBT document: {e}", path.display()));
    let tag = named.tag;
    let size = blueprint_size(&tag);
    let Some(Tag::List(palette)) = tag.get("palette") else {
        panic!("{}: no palette", path.display())
    };
    let states: Vec<(String, BTreeMap<String, String>)> = palette
        .iter()
        .map(|entry| {
            let Tag::Compound(entry) = entry else {
                panic!("{}: a palette entry is not a compound", path.display())
            };
            let name = match entry.get(&"Name".into()) {
                Some(Tag::String(name)) => name.to_string_lossy(),
                other => panic!("{}: palette entry without Name: {other:?}", path.display()),
            };
            let properties = match entry.get(&"Properties".into()) {
                Some(Tag::Compound(properties)) => properties
                    .iter()
                    .map(|(key, value)| match value {
                        Tag::String(value) => (key.to_string_lossy(), value.to_string_lossy()),
                        other => panic!(
                            "{}: property {key:?} is not a string: {other:?}",
                            path.display()
                        ),
                    })
                    .collect(),
                _ => BTreeMap::new(),
            };
            (name, properties)
        })
        .collect();
    let Some(Tag::IntArray(blocks)) = tag.get("blocks") else {
        panic!("{}: no blocks array", path.display())
    };
    let [sx, sy, sz] = size;
    let volume = (sx * sy * sz) as usize;
    assert_eq!(
        blocks.len(),
        volume.div_ceil(2),
        "{}: {} ints for {volume} cells",
        path.display(),
        blocks.len()
    );
    let cells = (0..volume)
        .map(|cell| {
            let word = blocks[cell / 2] as u32;
            let index = if cell % 2 == 0 {
                (word >> 16) & 0xffff
            } else {
                word & 0xffff
            } as usize;
            let (name, properties) = states
                .get(index)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: cell {cell} names palette entry {index} of {}",
                        path.display(),
                        states.len()
                    )
                })
                .clone();
            let y = cell as i32 / (sx * sz);
            let rest = cell as i32 % (sx * sz);
            let (z, x) = (rest / sx, rest % sx);
            Cell {
                pos: [x, y, z],
                name,
                properties,
            }
        })
        .collect();
    (size, cells)
}

/// What `StructureTemplate.transform(pos, Mirror.None, Rotation.CLOCKWISE_90,
/// pivot)` adds to a cell: `(x, y, z)` becomes `(-z, y, x)` with the settings'
/// pivot, which every caller leaves at zero (`processor.rs:954`). Written out
/// here rather than called, so the test does not check the engine with itself.
fn rotate_clockwise_90(cell: &Cell) -> [i32; 3] {
    let [x, y, z] = cell.pos;
    [-z, y, x]
}

/// `BlockState.rotate(CLOCKWISE_90)` for the states this blueprint holds, as
/// vanilla defines it: `StairsBlock.rotate` turns `facing` and nothing else
/// (`shape` is a question of neighbourhood, and `knownShape` keeps the
/// template's), an `axis` property follows the turned direction, and a block
/// that names its faces (`north`/`east`/`south`/`west`, fences and walls) takes
/// on each face what it had a quarter turn anticlockwise.
fn state_clockwise_90(
    name: &str,
    properties: &BTreeMap<String, String>,
) -> (String, BTreeMap<String, String>) {
    fn turn(direction: &str) -> &str {
        match direction {
            "north" => "east",
            "east" => "south",
            "south" => "west",
            "west" => "north",
            other => other,
        }
    }
    assert!(
        !properties.contains_key("rotation"),
        "{name}: a `rotation` (0..15) property is not modelled by this test"
    );
    let mut turned = BTreeMap::new();
    for (key, value) in properties {
        match key.as_str() {
            "facing" => {
                turned.insert(key.clone(), turn(value).to_owned());
            }
            "axis" => {
                let value = match value.as_str() {
                    "x" => "z",
                    "z" => "x",
                    other => other,
                };
                turned.insert(key.clone(), value.to_owned());
            }
            "north" | "east" | "south" | "west" => {
                let from = match key.as_str() {
                    "north" => "west",
                    "east" => "north",
                    "south" => "east",
                    _ => "south",
                };
                let value = properties
                    .get(from)
                    .unwrap_or_else(|| panic!("{name}: no `{from}` beside `{key}`"));
                turned.insert(key.clone(), value.clone());
            }
            _ => {
                turned.insert(key.clone(), value.to_owned());
            }
        }
    }
    (name.to_owned(), turned)
}

/// The state the world holds at `pos`, as the registry names it.
fn world_state(game: &Game<'_>, pos: [i32; 3]) -> (String, BTreeMap<String, String>) {
    let registry = game.pack.assets.registry;
    let id = game.world.block(pos[0], pos[1], pos[2]);
    let state = registry
        .state(id)
        .unwrap_or_else(|| panic!("{pos:?}: state {} is not in the registry", id.0));
    (state.name.clone(), BTreeMap::from(state.properties.clone()))
}

/// The registry's id for a complete state, failing by name on an unknown block,
/// property or value.
fn state_id(
    game: &Game<'_>,
    name: &str,
    properties: &BTreeMap<String, String>,
) -> horizon_world::registry::BlockStateId {
    let registry = game.pack.assets.registry;
    let descriptor = BlockState {
        name: name.to_owned(),
        properties: properties
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    };
    registry
        .resolve(&descriptor)
        .unwrap_or_else(|e| panic!("{name} {properties:?}: {e}"))
}

#[test]
#[ignore = "daily"]
fn a_converted_blueprint_places_and_rotates_cell_by_cell() {
    let Some((bp2nbt, corpus)) = converter_inputs() else {
        return;
    };
    let blueprint = corpus.join(BLUEPRINT);
    assert!(
        blueprint.is_file(),
        "the corpus has no {}",
        blueprint.display()
    );
    let (size, cells) = read_blueprint(&blueprint);
    let [sx, sy, sz] = size;
    assert_eq!(size, [5, 4, 4], "the blueprint's own size");
    assert_eq!(cells.len(), 80, "5x4x4 cells");

    with_game(|game| {
        // The game fixture opens no world of its own: the manager reads
        // templates under `world_root`, so give this test one to write into.
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.install_live_features();
        let scratch = tempfile::tempdir().unwrap();
        game.world_root = scratch.path().join("world");
        std::fs::create_dir_all(&game.world_root).unwrap();

        // 1. The converter, as the crate ships it: `bp2nbt <in> <out>`.
        let template_file = game
            .world_root
            .join("generated/test/structures/blueprint_nbt.nbt");
        std::fs::create_dir_all(template_file.parent().unwrap()).unwrap();
        let converted = std::process::Command::new(&bp2nbt)
            .arg(&blueprint)
            .arg(&template_file)
            .output()
            .unwrap_or_else(|e| panic!("{}: {e}", bp2nbt.display()));
        assert!(
            converted.status.success(),
            "bp2nbt failed: {}\n{}",
            String::from_utf8_lossy(&converted.stdout),
            String::from_utf8_lossy(&converted.stderr)
        );
        assert!(template_file.is_file(), "bp2nbt wrote no template");
        println!(
            "converted {} → {} ({} bytes)",
            blueprint.display(),
            template_file.display(),
            std::fs::metadata(&template_file).unwrap().len()
        );

        // 2. The manager's own load path: the file just written, under the
        // world's `generated/<ns>/structures/`, read as the engine's own
        // `DataVersion`.
        game.structure_templates.clear();
        let entry = game
            .template_entry(TEMPLATE)
            .unwrap_or_else(|| panic!("{TEMPLATE}: the manager did not load the template"));
        assert_eq!(template_size(&entry.tag), size, "template size");
        assert_eq!(
            entry.tag.get("DataVersion"),
            Some(&Tag::Int(horizon_core::version::DATA_VERSION)),
            "bp2nbt writes the engine's own data version"
        );

        // 3. A volume of stone under both boxes, so every air cell of the
        // blueprint has to be written to be read back as air.
        let stone = state_id(game, "minecraft:stone", &BTreeMap::new());
        let ([lo_x, lo_y, lo_z], [hi_x, hi_y, hi_z]) = STONE_BOX;
        for x in lo_x..=hi_x {
            for y in lo_y..=hi_y {
                for z in lo_z..=hi_z {
                    game.world.set(x, y, z, stone).unwrap();
                }
            }
        }
        for pos in [
            ORIGIN,
            [ORIGIN[0] + sx - 1, ORIGIN[1] + sy - 1, ORIGIN[2] + sz - 1],
            ROTATED_ORIGIN,
        ] {
            assert_eq!(
                game.world.block(pos[0], pos[1], pos[2]),
                stone,
                "{pos:?} was not stone before the placement"
            );
        }

        // 4. `StructureTemplate.placeInWorld` with `knownShape` (the command's
        // `strict`), integrity 1: an exact copy, no neighbour shape update.
        assert!(game.place_template_entry(
            &entry,
            BlockPos::new(ORIGIN[0], ORIGIN[1], ORIGIN[2]),
            load_settings(Mirror::None, Rotation::None, 1.0, 7, true),
            Mirror::None,
            Rotation::None,
        ));

        // 5. Every cell, not a sample.
        let mut air = 0;
        let mut off_default = 0;
        for cell in &cells {
            let pos = [
                ORIGIN[0] + cell.pos[0],
                ORIGIN[1] + cell.pos[1],
                ORIGIN[2] + cell.pos[2],
            ];
            let expected = state_id(game, &cell.name, &cell.properties);
            assert_eq!(
                world_state(game, pos),
                (cell.name.clone(), cell.properties.clone()),
                "cell {:?} of the blueprint, placed at {pos:?}",
                cell.pos
            );
            assert_eq!(
                game.world.block(pos[0], pos[1], pos[2]),
                expected,
                "cell {:?} at {pos:?}",
                cell.pos
            );
            if cell.name == "minecraft:air" {
                air += 1;
            } else {
                // The blueprint's own properties, not the block's defaults.
                let default = game
                    .pack
                    .assets
                    .registry
                    .default_state(&cell.name)
                    .expect("a default state");
                if expected != default {
                    off_default += 1;
                }
            }
        }
        assert!(
            air > 0,
            "the blueprint has air cells, so the volume was cleared"
        );
        assert!(
            off_default > 0,
            "some cells carry properties off their block's defaults"
        );
        println!(
            "{} cells: {} air, {} off the block's default state",
            cells.len(),
            air,
            off_default
        );

        // 6. The same template a quarter turn clockwise, four blocks up.
        assert!(game.place_template_entry(
            &entry,
            BlockPos::new(ROTATED_ORIGIN[0], ROTATED_ORIGIN[1], ROTATED_ORIGIN[2]),
            load_settings(Mirror::None, Rotation::Clockwise90, 1.0, 7, true),
            Mirror::None,
            Rotation::Clockwise90,
        ));
        // `transform` turns the box: `[size_z, size_y, size_x]`, whose minimum
        // corner is the origin less `size_z - 1` on x.
        assert_eq!(
            [
                ROTATED_ORIGIN[0] - (sz - 1),
                ROTATED_ORIGIN[1],
                ROTATED_ORIGIN[2]
            ],
            [5, 68, 8],
            "the rotated box's minimum corner"
        );
        let mut turned_cells = 0;
        for cell in &cells {
            let relative = rotate_clockwise_90(cell);
            let pos = [
                ROTATED_ORIGIN[0] + relative[0],
                ROTATED_ORIGIN[1] + relative[1],
                ROTATED_ORIGIN[2] + relative[2],
            ];
            let turned = state_clockwise_90(&cell.name, &cell.properties);
            assert_eq!(
                world_state(game, pos),
                turned.clone(),
                "cell {:?} turned a quarter clockwise, at {pos:?}",
                cell.pos
            );
            if turned != (cell.name.clone(), cell.properties.clone()) {
                turned_cells += 1;
            }
        }
        // A rotation that was ignored would have left the stairs facing their
        // blueprint directions: four of the seven palette entries change.
        assert!(
            turned_cells >= 4,
            "only {turned_cells} cells changed state under the rotation"
        );
        // Outside the turned box, the stone both boxes were filled with is
        // untouched.
        for pos in [
            [ROTATED_ORIGIN[0] - sz, ROTATED_ORIGIN[1], ROTATED_ORIGIN[2]],
            [ROTATED_ORIGIN[0] + 1, ROTATED_ORIGIN[1], ROTATED_ORIGIN[2]],
            [ROTATED_ORIGIN[0], ROTATED_ORIGIN[1], ROTATED_ORIGIN[2] - 1],
            [ROTATED_ORIGIN[0], ROTATED_ORIGIN[1], ROTATED_ORIGIN[2] + sx],
        ] {
            assert_eq!(
                game.world.block(pos[0], pos[1], pos[2]),
                stone,
                "{pos:?} is outside the rotated box and must still be stone"
            );
        }
        println!("{turned_cells} cells changed state under the quarter turn");
    });
}
