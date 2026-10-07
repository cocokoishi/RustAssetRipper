//! CRC32 animation-path recovery from `PathChecksumCache`.
//!
//! Ports the game-data-free kernels of
//! `AssetRipper.Processing/AnimationClips/PathChecksumCache.cs`: the two-way
//! checksum cache seeded with the empty path, hash-and-store `Add`, reverse
//! `TryGetPath`, clearing `Reset`, and the depth-cutoff field-path recursion.
//! Checksums are the reflected CRC-32 words of `Crc32Algorithm.HashUTF8`, reused
//! from [`crate::resources::blend_shape::crc32_ieee`] (pinned against the shipped
//! `AssetRipper.Checksum.dll` by `crc28_fixture.txt`).
//!
//! The population walks stay caller-side: hierarchy and avatar-table walks need the
//! asset collection, and `Add(IMonoScript)` needs live managed assemblies for
//! `GetBehaviourType`. Field shapes arrive as caller facts.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display};

use super::blend_shape::crc32_ieee;

/// The recursion depth at which `AddFieldRecursively` stops, mirroring the
/// `depth > 10` early return that prevents infinite recursion.
pub const MAX_FIELD_DEPTH: u32 = 10;

/// Bounds for the checksum cache, which the source grows without limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathChecksumLimits {
    /// Maximum cached paths across both lookup directions.
    pub max_entries: usize,
}

impl Default for PathChecksumLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_000_000,
        }
    }
}

/// Why a path could not enter the checksum cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathChecksumError {
    /// The cache already holds the maximum number of entries.
    EntryLimitExceeded { limit: usize },
    /// A bounded allocation for the cache could not be reserved.
    AllocationFailed,
}

impl Display for PathChecksumError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryLimitExceeded { limit } => write!(
                formatter,
                "path checksum cache holds more than the limit of {limit} entries"
            ),
            Self::AllocationFailed => {
                write!(formatter, "could not allocate path checksum cache storage")
            }
        }
    }
}

impl Error for PathChecksumError {}

/// One serializable field shape for the path recursion: its name, whether it is a
/// primitive (only primitives can be animated), and its nested fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChecksumField {
    /// `SerializableType.Field.Name`.
    pub name: String,
    /// `field.Type.IsPrimitive()`.
    pub is_primitive: bool,
    /// `field.Type.Fields`.
    pub children: Vec<ChecksumField>,
}

/// The two-way checksum cache of `PathChecksumCache`, seeded with the empty path.
///
/// Insertions beyond [`PathChecksumLimits::max_entries`] fail explicitly instead of
/// growing without bound like the source dictionary.
#[derive(Clone, Debug, Default)]
pub struct PathChecksumCache {
    property_names: HashMap<String, u32>,
    checksums: HashMap<u32, String>,
    limits: PathChecksumLimits,
}

impl PathChecksumCache {
    /// An empty cache is impossible in the source: construction seeds both maps with
    /// the empty path, so this constructor does the same.
    pub fn new(limits: PathChecksumLimits) -> Result<Self, PathChecksumError> {
        let mut cache = Self {
            property_names: HashMap::new(),
            checksums: HashMap::new(),
            limits,
        };
        cache.insert("", 0)?;
        Ok(cache)
    }

    /// `Add(string)`: a cached path returns its stored checksum, otherwise the CRC-32
    /// of the UTF-8 path is stored in both directions and returned.
    pub fn add(&mut self, path: &str) -> Result<u32, PathChecksumError> {
        if let Some(checksum) = self.property_names.get(path) {
            return Ok(*checksum);
        }
        let checksum = crc32_ieee(path.as_bytes());
        self.insert(path, checksum)?;
        Ok(checksum)
    }

    /// `TryGetPath`: the reverse lookup of a checksum, if a stored path produced it.
    pub fn try_get_path(&self, identifier: u32) -> Option<&str> {
        self.checksums.get(&identifier).map(String::as_str)
    }

    /// `Reset`: clears both maps and the processed set. Like the source, the
    /// empty-path seed is cleared too and is not re-added.
    pub fn reset(&mut self) {
        self.property_names.clear();
        self.checksums.clear();
    }

    /// `AddFieldRecursively` over caller-supplied field shapes: primitive fields add
    /// `path + name`, complex fields recurse into `path + name + "."`, and recursion
    /// past [`MAX_FIELD_DEPTH`] returns without adding, mirroring the source cutoff.
    pub fn add_fields(
        &mut self,
        fields: &[ChecksumField],
        base_path: &str,
        depth: u32,
    ) -> Result<(), PathChecksumError> {
        if depth > MAX_FIELD_DEPTH {
            return Ok(());
        }
        for field in fields {
            if field.is_primitive {
                let path = format!("{base_path}{}", field.name);
                self.add(&path)?;
            } else {
                let nested = format!("{base_path}{}.", field.name);
                self.add_fields(&field.children, &nested, depth + 1)?;
            }
        }
        Ok(())
    }

    fn insert(&mut self, path: &str, checksum: u32) -> Result<(), PathChecksumError> {
        if self.property_names.len() >= self.limits.max_entries {
            return Err(PathChecksumError::EntryLimitExceeded {
                limit: self.limits.max_entries,
            });
        }
        self.property_names
            .try_reserve(1)
            .map_err(|_| PathChecksumError::AllocationFailed)?;
        self.checksums
            .try_reserve(1)
            .map_err(|_| PathChecksumError::AllocationFailed)?;
        self.property_names.insert(path.to_owned(), checksum);
        self.checksums.insert(checksum, path.to_owned());
        Ok(())
    }
}

/// Tracks already-visited script identities for the caller-side `Add(IMonoScript)`
/// walk, mirroring the `processedAssets` set that needs live assemblies.
#[derive(Clone, Debug, Default)]
pub struct ProcessedScriptSet {
    identities: HashSet<String>,
}

impl ProcessedScriptSet {
    /// Records one script identity; returns false when it was already present,
    /// mirroring the early return of `Add(IMonoScript)`.
    pub fn insert(&mut self, identity: &str) -> bool {
        self.identities.insert(identity.to_owned())
    }

    /// `Reset` also clears the processed set.
    pub fn clear(&mut self) {
        self.identities.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        crc32_ieee, ChecksumField, PathChecksumCache, PathChecksumError, PathChecksumLimits,
    };

    fn cache() -> PathChecksumCache {
        PathChecksumCache::new(PathChecksumLimits::default()).unwrap()
    }

    fn leaf(name: &str) -> ChecksumField {
        ChecksumField {
            name: name.to_owned(),
            is_primitive: true,
            children: Vec::new(),
        }
    }

    fn node(name: &str, children: Vec<ChecksumField>) -> ChecksumField {
        ChecksumField {
            name: name.to_owned(),
            is_primitive: false,
            children,
        }
    }

    #[test]
    fn empty_path_is_seeded_in_both_directions() {
        let cache = cache();
        assert_eq!(cache.try_get_path(0), Some(""));
        // The seed round-trips through `Add` without rehashing.
        let mut cache = cache;
        assert_eq!(cache.add("").unwrap(), 0);
    }

    #[test]
    fn add_hashes_and_reverse_lookup_round_trips() {
        let mut cache = cache();
        let checksum = cache.add("Root/Hip").unwrap();
        assert_eq!(checksum, crc32_ieee("Root/Hip".as_bytes()));
        assert_eq!(cache.try_get_path(checksum), Some("Root/Hip"));
        // A second add returns the stored checksum.
        assert_eq!(cache.add("Root/Hip").unwrap(), checksum);
        assert_eq!(cache.try_get_path(0xDEAD_BEEF), None);
    }

    #[test]
    fn reset_clears_both_maps_including_the_seed() {
        let mut cache = cache();
        let checksum = cache.add("Root/Hip").unwrap();
        cache.reset();
        assert_eq!(cache.try_get_path(0), None);
        assert_eq!(cache.try_get_path(checksum), None);
        // The cache stays usable after a reset.
        assert_eq!(cache.add("Root/Hip").unwrap(), checksum);
    }

    /// Differential against the real `PathChecksumCache` methods committed in
    /// `anim_cache_fixture.txt` (probe `Temp/opencode/animcacheprobe`, isolated
    /// reference bins, SDK 10.0.401): every `Add` word, every reverse lookup,
    /// the miss, and the reset-then-re-add sequence reproduce exactly.
    #[test]
    fn cache_reproduces_the_reference_add_get_reset_sequence() {
        let fixture = include_str!("anim_cache_fixture.txt");
        let mut cache = cache();
        let mut rows = 0;
        let mut reset_seen = false;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "SEED" => {
                    assert_eq!(cache.try_get_path(0), Some(""), "{line}");
                }
                "ADD" | "READD" | "POSTRESETADD" => {
                    assert_eq!(parts.len(), 3, "{line}");
                    let expected: u32 = parts[2].parse().unwrap();
                    assert_eq!(cache.add(parts[1]).unwrap(), expected, "{line}");
                    rows += 1;
                }
                "GET" | "POSTRESETGET" => {
                    assert_eq!(parts.len(), 4, "{line}");
                    let identifier: u32 = parts[1].parse().unwrap();
                    assert_eq!(cache.try_get_path(identifier), Some(parts[3]), "{line}");
                    rows += 1;
                }
                "MISS" => {
                    // Normalized from the probe line: the queried identifier in
                    // decimal plus whether a path was found.
                    assert_eq!(parts.len(), 3, "{line}");
                    let identifier: u32 = parts[1].parse().unwrap();
                    assert_eq!(cache.try_get_path(identifier), None, "{line}");
                    assert_eq!(parts[2], "False", "{line}");
                }
                "POSTRESET" => {
                    cache.reset();
                    reset_seen = true;
                    assert_eq!(cache.try_get_path(0), None, "{line}");
                }
                kind => panic!("unknown fixture row {kind}: {line}"),
            }
        }
        assert!(reset_seen, "the fixture pins the reset sequence");
        assert!(rows >= 18, "the fixture pins adds and reverse lookups");
    }

    #[test]
    fn field_recursion_builds_dotted_primitive_paths() {
        let mut cache = cache();
        let fields = vec![
            leaf("m_LocalPosition"),
            node(
                "m_Transform",
                vec![leaf("x"), node("deep", vec![leaf("y")])],
            ),
        ];
        cache.add_fields(&fields, "", 0).unwrap();
        assert!(cache.try_get_path(crc32_ieee(b"m_LocalPosition")).is_some());
        assert!(cache.try_get_path(crc32_ieee(b"m_Transform.x")).is_some());
        assert!(cache
            .try_get_path(crc32_ieee(b"m_Transform.deep.y"))
            .is_some());
    }

    #[test]
    fn field_recursion_stops_past_the_depth_cutoff() {
        // Eleven nested structs put the leaf at depth 11, past the cutoff.
        let mut deep = leaf("value");
        for level in (0..11).rev() {
            deep = node(&format!("n{level}"), vec![deep]);
        }
        let mut cache = cache();
        cache.add_fields(&[deep], "", 0).unwrap();
        assert_eq!(
            cache.try_get_path(crc32_ieee(b"n0.n1.n2.n3.n4.n5.n6.n7.n8.n9.n10.value")),
            None
        );

        // Ten nested structs put the leaf exactly at depth 10, which still adds.
        let mut shallow = leaf("value");
        for level in (0..10).rev() {
            shallow = node(&format!("m{level}"), vec![shallow]);
        }
        cache.add_fields(&[shallow], "", 0).unwrap();
        assert!(cache
            .try_get_path(crc32_ieee(b"m0.m1.m2.m3.m4.m5.m6.m7.m8.m9.value"))
            .is_some());
    }

    #[test]
    fn entry_limit_is_an_explicit_error() {
        let mut cache = PathChecksumCache::new(PathChecksumLimits { max_entries: 1 }).unwrap();
        // The seed already fills the single slot.
        assert_eq!(
            cache.add("Root/Hip"),
            Err(PathChecksumError::EntryLimitExceeded { limit: 1 })
        );
    }
}
