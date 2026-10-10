use super::super::managed_registry::ManagedReferenceTypeTreeResolver;
use super::super::metadata::{ObjectInfo, SerializedTypeReference};
use super::decode_error::{ObjectDecodeError, ObjectDecodeErrorKind};
use super::type_tree::MAX_TYPE_TREE_DEPTH;

/// Per-object bounds for dynamic TypeTree decoding. Callers can raise these limits when
/// processing trusted large assets or lower them for untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectDecodeLimits {
    pub max_decoded_allocation_bytes: usize,
    pub max_array_elements: usize,
    pub max_string_bytes: usize,
    pub max_type_tree_depth: usize,
    /// Maximum number of managed-reference registries recursively nested in payload data.
    pub max_managed_registry_depth: usize,
}

impl Default for ObjectDecodeLimits {
    fn default() -> Self {
        Self {
            max_decoded_allocation_bytes: 256 * 1024 * 1024,
            max_array_elements: 16 * 1024 * 1024,
            max_string_bytes: 64 * 1024 * 1024,
            max_type_tree_depth: MAX_TYPE_TREE_DEPTH,
            max_managed_registry_depth: 64,
        }
    }
}

pub(super) struct DecodeBudget<'a> {
    pub(super) limits: &'a ObjectDecodeLimits,
    pub(super) allocated_bytes: usize,
    pub(super) managed_type_references: &'a [SerializedTypeReference],
    pub(super) managed_reference_type_resolver: Option<&'a dyn ManagedReferenceTypeTreeResolver>,
    pub(super) stream_offset: usize,
    pub(super) managed_registry_depth: usize,
}

#[derive(Clone, Copy)]
pub(super) struct ObjectDecodeTarget<'a> {
    pub(super) index: usize,
    pub(super) object: &'a ObjectInfo,
    pub(super) fallback_type_id: i32,
}

pub(crate) struct ManagedPayloadDecodeContext<'a> {
    pub entry_index: usize,
    pub stream_offset: usize,
    pub managed_registry_depth: usize,
    pub managed_reference_type_resolver: Option<&'a dyn ManagedReferenceTypeTreeResolver>,
}

impl DecodeBudget<'_> {
    pub(super) fn charge(
        &mut self,
        requested: usize,
        offset: usize,
        resource: &str,
    ) -> Result<(), ObjectDecodeError> {
        let total = self.allocated_bytes.checked_add(requested).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::AllocationSizeOverflow {
                    resource: resource.to_owned(),
                },
            )
        })?;
        if total > self.limits.max_decoded_allocation_bytes {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::DecodeLimitExceeded {
                    resource: resource.to_owned(),
                    requested: total,
                    limit: self.limits.max_decoded_allocation_bytes,
                },
            ));
        }
        self.allocated_bytes = total;
        Ok(())
    }

    pub(super) fn charge_items<T>(
        &mut self,
        count: usize,
        offset: usize,
        resource: &str,
    ) -> Result<(), ObjectDecodeError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::AllocationSizeOverflow {
                    resource: resource.to_owned(),
                },
            )
        })?;
        self.charge(bytes, offset, resource)
    }

    pub(super) fn remaining_allocation_bytes(&self) -> usize {
        self.limits
            .max_decoded_allocation_bytes
            .saturating_sub(self.allocated_bytes)
    }
}
