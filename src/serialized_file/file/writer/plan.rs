use super::metadata::MetadataRewritePlan;
use super::request::ValidatedRewriteRequest;

pub(super) struct SerializedFileRewritePlan<'input, 'request> {
    pub(super) request: ValidatedRewriteRequest<'request>,
    pub(super) metadata: MetadataRewritePlan<'input>,
}
