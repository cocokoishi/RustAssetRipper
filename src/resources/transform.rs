//! `TransformExtensions` hierarchy helpers.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/TransformExtensions.cs` for the root-path,
//! child-lookup, and sibling-order behaviors. The Rust model uses an arena of transform nodes so
//! the hierarchy can be walked without owning asset objects.

/// A failure while walking a transform hierarchy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformError {
    /// The parent chain contained a cycle.
    Cycle,
    /// The transform was not found among its father's children.
    NotAmongFathersChildren,
}

impl core::fmt::Display for TransformError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cycle => write!(formatter, "the transform hierarchy contains a cycle"),
            Self::NotAmongFathersChildren => {
                write!(
                    formatter,
                    "the transform is not among its father's children"
                )
            }
        }
    }
}

impl std::error::Error for TransformError {}

/// A transform node in an arena.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransformNode {
    /// The attached GameObject's name, or `None` when no GameObject resolves.
    pub name: Option<String>,
    /// The father's arena index.
    pub father: Option<usize>,
    /// The children's arena indices, in serialized order.
    pub children: Vec<usize>,
}

/// `TransformExtensions.GetRootPath`.
///
/// The reference recurses without a cycle guard; this walks iteratively and reports a cycle so a
/// malformed hierarchy cannot exhaust the stack.
pub fn get_root_path(tree: &[TransformNode], index: usize) -> Result<String, TransformError> {
    let mut names: Vec<&str> = Vec::new();
    let mut current = index;
    let mut steps = 0usize;
    loop {
        names.push(tree[current].name.as_deref().unwrap_or("Unnamed"));
        match tree[current].father {
            Some(father) => {
                steps += 1;
                if steps > tree.len() {
                    return Err(TransformError::Cycle);
                }
                current = father;
            }
            None => break,
        }
    }
    names.reverse();
    Ok(names.join("/"))
}

/// `TransformExtensions.FindChild`.
pub fn find_child(tree: &[TransformNode], index: usize, path: &str) -> Option<usize> {
    if path.is_empty() {
        return Some(index);
    }
    find_child_from(tree, index, path, 0)
}

fn find_child_from(
    tree: &[TransformNode],
    index: usize,
    path: &str,
    start: usize,
) -> Option<usize> {
    let rest = &path[start..];
    let (name, next) = match rest.find('/') {
        Some(position) => (&rest[..position], Some(start + position + 1)),
        None => (rest, None),
    };
    for &child in &tree[index].children {
        if tree[child].name.as_deref() == Some(name) {
            return match next {
                Some(next_start) => find_child_from(tree, child, path, next_start),
                None => Some(child),
            };
        }
    }
    None
}

/// `TransformExtensions.CalculateRootOrder`.
pub fn calculate_root_order(tree: &[TransformNode], index: usize) -> Result<usize, TransformError> {
    let Some(father) = tree[index].father else {
        return Ok(0);
    };
    for (position, child) in tree[father].children.iter().enumerate().rev() {
        if *child == index {
            return Ok(position);
        }
    }
    Err(TransformError::NotAmongFathersChildren)
}

/// A transform's serialized local fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformLocalView {
    /// `LocalPosition`.
    pub position: [f32; 3],
    /// `LocalRotation` (x, y, z, w).
    pub rotation: [f32; 4],
    /// `LocalScale`.
    pub scale: [f32; 3],
    /// `RootOrder`.
    pub root_order: i32,
    /// `LocalEulerAnglesHint`: present only on versions that serialize it.
    pub euler_hint: Option<[f32; 3]>,
}

impl TransformLocalView {
    /// `TransformExtensions.InitializeDefault`.
    pub fn initialize_default(&mut self) {
        self.position = [0.0; 3];
        self.rotation = [0.0, 0.0, 0.0, 1.0];
        self.scale = [1.0; 3];
        self.root_order = 0;
        if let Some(hint) = &mut self.euler_hint {
            *hint = [0.0; 3];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<TransformNode> {
        vec![
            TransformNode {
                name: None,
                father: None,
                children: Vec::new(),
            },
            TransformNode {
                name: Some("Root".to_string()),
                father: None,
                children: vec![2, 4],
            },
            TransformNode {
                name: Some("Child".to_string()),
                father: Some(1),
                children: vec![3],
            },
            TransformNode {
                name: Some("Grand".to_string()),
                father: Some(2),
                children: Vec::new(),
            },
            TransformNode {
                name: Some("Sibling".to_string()),
                father: Some(1),
                children: Vec::new(),
            },
        ]
    }

    /// Byte-for-byte differential against the reference `TransformExtensions` (probe
    /// `Temp/opencode/goprobe2`).
    #[test]
    fn transform_hierarchy_matches_the_reference_runtime() {
        let tree = tree();
        let fixture = include_str!("transform_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "TR", "{line}");
            assert_eq!(get_root_path(&tree, 0).unwrap(), parts[2], "{line}");
            assert_eq!(get_root_path(&tree, 1).unwrap(), parts[3], "{line}");
            assert_eq!(get_root_path(&tree, 2).unwrap(), parts[4], "{line}");
            assert_eq!(get_root_path(&tree, 3).unwrap(), parts[5], "{line}");
            assert_eq!(
                find_child(&tree, 1, "") == Some(1),
                parts[6] == "1",
                "{line}"
            );
            assert_eq!(
                find_child(&tree, 1, "Child") == Some(2),
                parts[7] == "1",
                "{line}"
            );
            assert_eq!(
                find_child(&tree, 1, "Child/Grand") == Some(3),
                parts[8] == "1",
                "{line}"
            );
            assert_eq!(
                find_child(&tree, 1, "Missing").is_none(),
                parts[9] == "1",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);

        // A second child shares the father, so its root order is one.
        assert_eq!(calculate_root_order(&tree, 4), Ok(1));
        assert_eq!(calculate_root_order(&tree, 2), Ok(0));
        assert_eq!(calculate_root_order(&tree, 1), Ok(0));

        // A cycle is reported rather than overflowing.
        let cyclic = vec![TransformNode {
            name: Some("A".to_string()),
            father: Some(0),
            children: Vec::new(),
        }];
        assert_eq!(get_root_path(&cyclic, 0), Err(TransformError::Cycle));
    }

    /// Byte-for-byte differential against the reference `TransformExtensions.InitializeDefault`
    /// (probe `Temp/opencode/vecprobe`).
    #[test]
    fn transform_initialize_default_matches_the_reference_runtime() {
        let fixture = include_str!("transform_default_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "TD", "{line}");
            let has_euler = parts[11] == "1";
            let mut view = TransformLocalView {
                position: [9.0; 3],
                rotation: [9.0; 4],
                scale: [9.0; 3],
                root_order: 7,
                euler_hint: has_euler.then_some([9.0; 3]),
            };
            view.initialize_default();
            assert_eq!(
                [
                    view.position[0].to_bits(),
                    view.position[1].to_bits(),
                    view.position[2].to_bits(),
                ],
                [
                    parts[2].parse::<u32>().unwrap(),
                    parts[3].parse::<u32>().unwrap(),
                    parts[4].parse::<u32>().unwrap(),
                ],
                "{line}"
            );
            assert_eq!(
                view.rotation[3].to_bits(),
                parts[5].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.rotation[0].to_bits(),
                parts[6].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                [
                    view.scale[0].to_bits(),
                    view.scale[1].to_bits(),
                    view.scale[2].to_bits(),
                ],
                [
                    parts[7].parse::<u32>().unwrap(),
                    parts[8].parse::<u32>().unwrap(),
                    parts[9].parse::<u32>().unwrap(),
                ],
                "{line}"
            );
            assert_eq!(view.root_order, parts[10].parse::<i32>().unwrap(), "{line}");
            assert_eq!(view.euler_hint.is_some(), has_euler, "{line}");
            if has_euler {
                assert_eq!(
                    view.euler_hint.unwrap()[0].to_bits(),
                    parts[12].parse::<u32>().unwrap(),
                    "{line}"
                );
            }
            rows += 1;
        }
        assert_eq!(rows, 3);
    }
}
