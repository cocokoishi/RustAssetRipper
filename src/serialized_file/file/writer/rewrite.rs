use super::bytes::copy_input;
use super::execution;
use super::metadata::MetadataRewritePlan;
use super::model::{
    SerializedExternalFileInsertion, SerializedFileRewriteError, SerializedFileRewriteLimits,
    SerializedObjectInsertion,
};
use super::plan::SerializedFileRewritePlan;
use super::request;
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::object::ParsedObjectPayload;

impl SerializedFile<'_> {
    /// Rebuild the complete file with zero or more replaced object payloads.
    ///
    /// Original metadata bytes and non-object metadata records are retained. Object records,
    /// data-section offsets, versioned headers, and alignment padding are updated for the new
    /// payload lengths. With an empty replacement list, the original bytes are copied unchanged.
    pub fn rewrite_object_payloads(
        &self,
        replacements: &[(usize, ParsedObjectPayload)],
        limits: SerializedFileRewriteLimits,
    ) -> Result<Vec<u8>, SerializedFileRewriteError> {
        self.rewrite_with_insertions(replacements, &[], limits)
    }

    /// Rebuild the file with object payload replacements and new records using existing types.
    ///
    /// Inserted objects are appended after the original object table and their payloads are
    /// placed after all original payloads. Metadata after the object table is preserved exactly.
    pub fn rewrite_with_insertions(
        &self,
        replacements: &[(usize, ParsedObjectPayload)],
        insertions: &[SerializedObjectInsertion],
        limits: SerializedFileRewriteLimits,
    ) -> Result<Vec<u8>, SerializedFileRewriteError> {
        self.rewrite_with_changes(replacements, insertions, &[], limits)
    }

    /// Rebuild the file with payload edits, object insertions, and external-file table appends.
    ///
    /// External identifiers are inserted after existing external records and before any
    /// following reference-type or user-information metadata. Generation-specific identifier
    /// fields are validated and encoded from `FileIdentifier`.
    pub fn rewrite_with_changes(
        &self,
        replacements: &[(usize, ParsedObjectPayload)],
        insertions: &[SerializedObjectInsertion],
        external_file_insertions: &[SerializedExternalFileInsertion],
        limits: SerializedFileRewriteLimits,
    ) -> Result<Vec<u8>, SerializedFileRewriteError> {
        let total_object_count = request::validate_counts(
            self,
            replacements,
            insertions,
            external_file_insertions,
            limits,
        )?;
        if replacements.is_empty() && insertions.is_empty() && external_file_insertions.is_empty() {
            return copy_input(self.input(), limits.max_output_file_bytes);
        }

        let request = request::validate_request(
            self,
            replacements,
            insertions,
            external_file_insertions,
            total_object_count,
        )?;
        let metadata = MetadataRewritePlan::prepare(
            self,
            request.insertions,
            request.external_file_insertions,
            limits,
        )?;
        let plan = SerializedFileRewritePlan { request, metadata };
        execution::execute(self, &plan, limits)
    }
}
