use super::model::{
    UnityYamlObjectConversionError, UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits,
};
pub(super) fn serialized_version_for_path(
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<i32>, UnityYamlObjectConversionError> {
    if let Some(version) = hints.serialized_version_paths.get(field_path) {
        return Ok(Some(*version));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.serialized_version_paths.get(&canonical_path).copied())
}

pub(super) fn canonical_array_path(
    path: &str,
    max_bytes: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let mut canonical = String::new();
    canonical.try_reserve_exact(path.len()).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML canonical field path",
            requested: path.len(),
        }
    })?;
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'[' {
            let mut digit_end = index + 1;
            while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
                digit_end += 1;
            }
            if digit_end > index + 1 && bytes.get(digit_end) == Some(&b']') {
                canonical.push_str("[*]");
                index = digit_end + 1;
                continue;
            }
        }
        let Some(character) = path[index..].chars().next() else {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "field path is not valid UTF-8",
            });
        };
        canonical.push(character);
        index += character.len_utf8();
    }
    if canonical.len() > max_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: canonical.len(),
            limit: max_bytes,
        });
    }
    Ok(canonical)
}

pub(super) fn child_field_path(
    parent: &str,
    name: &str,
    max_bytes: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let separator_bytes = usize::from(!parent.is_empty());
    let requested = parent
        .len()
        .checked_add(separator_bytes)
        .and_then(|length| length.checked_add(name.len()))
        .ok_or(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: max_bytes,
        })?;
    if requested > max_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: max_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML field path",
            requested,
        }
    })?;
    path.push_str(parent);
    if !parent.is_empty() {
        path.push('.');
    }
    path.push_str(name);
    Ok(path)
}

pub(super) fn indexed_field_path(
    parent: &str,
    index: usize,
    max_bytes: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let index = index.to_string();
    let requested = parent
        .len()
        .checked_add(index.len())
        .and_then(|length| length.checked_add(2))
        .ok_or(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: max_bytes,
        })?;
    if requested > max_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: max_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML array field path",
            requested,
        }
    })?;
    path.push_str(parent);
    path.push('[');
    path.push_str(&index);
    path.push(']');
    Ok(path)
}

pub(super) fn try_copy_string(
    value: &str,
    resource: &'static str,
) -> Result<String, UnityYamlObjectConversionError> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len()).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource,
            requested: value.len(),
        }
    })?;
    copy.push_str(value);
    Ok(copy)
}

pub(super) struct ConversionBudget {
    visited: usize,
    pub(super) limits: UnityYamlObjectConversionLimits,
}

impl ConversionBudget {
    pub(super) fn new(limits: UnityYamlObjectConversionLimits) -> Self {
        Self { visited: 0, limits }
    }

    pub(super) fn visit(&mut self, depth: usize) -> Result<(), UnityYamlObjectConversionError> {
        if depth > self.limits.max_depth {
            return Err(UnityYamlObjectConversionError::DepthLimitExceeded {
                requested: depth,
                limit: self.limits.max_depth,
            });
        }
        self.visited = self.visited.checked_add(1).ok_or(
            UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: usize::MAX,
                limit: self.limits.max_values,
            },
        )?;
        if self.visited > self.limits.max_values {
            return Err(UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: self.visited,
                limit: self.limits.max_values,
            });
        }
        Ok(())
    }

    pub(super) fn check_field_path(
        &self,
        path: &str,
    ) -> Result<(), UnityYamlObjectConversionError> {
        if path.len() > self.limits.max_field_path_bytes {
            return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
                requested: path.len(),
                limit: self.limits.max_field_path_bytes,
            });
        }
        Ok(())
    }
}

pub(super) fn wildcard_element_path(
    parent: &str,
    limits: UnityYamlObjectConversionLimits,
) -> Result<String, UnityYamlObjectConversionError> {
    let requested = parent.len().checked_add(3).ok_or(
        UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: limits.max_field_path_bytes,
        },
    )?;
    if requested > limits.max_field_path_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: limits.max_field_path_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML array schema path",
            requested,
        }
    })?;
    path.push_str(parent);
    path.push_str("[*]");
    Ok(path)
}
