use super::*;
pub(super) fn checked_charge(
    current: usize,
    amount: usize,
    limit: usize,
    resource: &'static str,
) -> Result<usize, WorkspaceSceneHierarchyError> {
    let requested = current
        .checked_add(amount)
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow { resource })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        });
    }
    Ok(requested)
}
