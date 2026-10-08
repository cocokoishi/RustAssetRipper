//! The generated `FieldHashes` table consulted by
//! `AnimationClipConverter.AddEngineCurve`.
//!
//! `AssetRipper.SourceGenerated.FieldHashes.TryGetPath(classID, hash, out path)` is a
//! class-id switch over one `s_FieldPathDictionary` per generated class group
//! (`AssetRipper.AssemblyDumper/Passes/Pass941_MakeFieldHashes.cs`: per-group CRC-32
//! dictionaries, an ID switch, and four merged-group aliases). The committed
//! `field_hashes_fixture.txt` is a direct dump of those shipped dictionaries taken from
//! the pinned reference runtime, so this module reproduces the same lookup without
//! re-deriving the generator.
//!
//! Fixture format (see the probe `tools/probes/anim-convert`):
//!
//! ```text
//! P|<pool index>|<path>
//! C|<class id>|<hash>:<pool index>,<hash>:<pool index>,...
//! ```
//!
//! The class rows are sorted by hash so the lookup is a binary search.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `Pass001_MergeMovedGroups` aliases: an unmerged class id resolves through the main
/// group id's dictionary, because the generated `FieldHashes` switch emits a case for
/// both ids pointing at the same method.
const MERGED_GROUP_ALIASES: &[(i32, i32)] = &[
    (194, 238),  // NavMesh
    (197, 258),  // LightProbes
    (1011, 319), // AvatarMask
    (327, 329),  // VideoClip
];

struct FieldHashes {
    pool: Vec<&'static str>,
    classes: HashMap<i32, Vec<(u32, usize)>>,
}

fn table() -> &'static FieldHashes {
    static TABLE: OnceLock<FieldHashes> = OnceLock::new();
    TABLE.get_or_init(|| {
        let fixture = include_str!("field_hashes_fixture.txt");
        let mut pool: Vec<&'static str> = Vec::new();
        let mut classes: HashMap<i32, Vec<(u32, usize)>> = HashMap::new();
        for line in fixture.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("P|") {
                let (index, path) = rest
                    .split_once('|')
                    .expect("a field-hash pool row carries an index and a path");
                let index: usize = index.parse().expect("a pool index");
                if index == pool.len() {
                    pool.push(path);
                } else {
                    assert!(index < pool.len(), "pool rows arrive in index order");
                }
            } else if let Some(rest) = line.strip_prefix("C|") {
                let (class_id, rows) = rest
                    .split_once('|')
                    .expect("a class row carries an id and its hash rows");
                let class_id: i32 = class_id.parse().expect("a class id");
                let mut entries = Vec::new();
                if !rows.is_empty() {
                    for row in rows.split(',') {
                        let (hash, index) = row.split_once(':').expect("a hash row");
                        entries.push((
                            hash.parse().expect("a hash word"),
                            index.parse::<usize>().expect("a pool index"),
                        ));
                    }
                }
                entries.sort_unstable_by_key(|(hash, _)| *hash);
                classes.insert(class_id, entries);
            }
        }
        FieldHashes { pool, classes }
    })
}

/// `FieldHashes.TryGetPath`: the field path for a class id and CRC-32 hash, if the
/// generated dictionary holds one. A missing class or hash yields `None`, matching the
/// generated `NullHelper` arm.
pub fn try_get_path(class_id: i32, hash: u32) -> Option<&'static str> {
    let mut id = class_id;
    for (alias, main) in MERGED_GROUP_ALIASES {
        if *alias == id {
            id = *main;
            break;
        }
    }
    let table = table();
    let entries = table.classes.get(&id)?;
    let position = entries
        .binary_search_by_key(&hash, |(word, _)| *word)
        .ok()?;
    table.pool.get(entries[position].1).copied()
}

/// The number of class dictionaries in the committed table; used by tests to keep the
/// fixture honest.
pub fn class_count() -> usize {
    table().classes.len()
}

/// The number of field paths in the committed pool.
pub fn pool_len() -> usize {
    table().pool.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::blend_shape::crc32_ieee;

    #[test]
    fn table_carries_every_dumped_class_and_path() {
        assert_eq!(class_count(), 391);
        assert_eq!(pool_len(), 7_464);
    }

    /// Spot rows read straight out of the shipped dictionaries: a Transform axis path on
    /// class 4, inherited `m_ObjectHideFlags` paths on renderer classes, and the merged
    /// NavMesh/AvatarMask group aliases.
    #[test]
    fn known_field_paths_resolve() {
        assert_eq!(
            try_get_path(4, crc32_ieee(b"m_LocalPosition.x")),
            Some("m_LocalPosition.x")
        );
        assert_eq!(
            try_get_path(25, crc32_ieee(b"m_ObjectHideFlags")),
            Some("m_ObjectHideFlags")
        );
        assert_eq!(
            try_get_path(23, crc32_ieee(b"m_ObjectHideFlags")),
            Some("m_ObjectHideFlags")
        );
        // The merged NavMesh group answers under both ids.
        let nav_mesh = try_get_path(238, crc32_ieee(b"m_ObjectHideFlags"));
        assert_eq!(
            try_get_path(194, crc32_ieee(b"m_ObjectHideFlags")),
            nav_mesh
        );
        assert!(nav_mesh.is_some());
        assert_eq!(
            try_get_path(1011, crc32_ieee(b"m_ObjectHideFlags")),
            try_get_path(319, crc32_ieee(b"m_ObjectHideFlags"))
        );
    }

    #[test]
    fn unknown_class_and_hash_miss() {
        assert_eq!(try_get_path(-1, 0), None);
        assert_eq!(try_get_path(4, 0xDEAD_BEEF), None);
        assert_eq!(try_get_path(0, 0), None);
    }

    /// Every pool row really is the CRC-32 of its own path, and every class row points
    /// at a pool entry whose hash matches: the fixture is internally consistent, so a
    /// truncated or reordered dump cannot silently pass.
    #[test]
    fn fixture_is_self_consistent() {
        let table = table();
        let mut entries = 0usize;
        for (class_id, rows) in &table.classes {
            assert!(!rows.is_empty(), "class {class_id} has rows");
            let mut previous = None;
            for (hash, index) in rows {
                assert!(previous.is_none_or(|word| word < *hash), "sorted rows");
                previous = Some(*hash);
                let path = table.pool[*index];
                assert_eq!(
                    crc32_ieee(path.as_bytes()),
                    *hash,
                    "pool row {index} ({path}) hashes to its recorded word"
                );
                entries += 1;
            }
        }
        assert_eq!(entries, 25_624);
    }
}
