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
}
