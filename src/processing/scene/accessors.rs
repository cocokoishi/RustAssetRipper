use super::*;

impl SceneGraph {
    pub fn objects(&self) -> &[SceneObjectRecord] {
        &self.objects
    }

    pub fn relationships(&self) -> &[SceneRelationship] {
        &self.relationships
    }

    pub fn issues(&self) -> &[SceneGraphIssue] {
        &self.issues
    }

    /// True when relevant TypeTrees decoded and every pointer uniquely resolves in the joined set.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
            && self.relationships.iter().all(|relationship| {
                matches!(
                    &relationship.target,
                    SceneRelationshipTarget::Null
                        | SceneRelationshipTarget::LocalObject { .. }
                        | SceneRelationshipTarget::WorkspaceObject { .. }
                )
            })
    }
}
