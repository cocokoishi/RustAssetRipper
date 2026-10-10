//! Per-SerializedFile object identity indexing and TypeTree-backed PPtr collection.

include!("reference_graph_models.rs");
include!("reference_graph_walkers.rs");

#[cfg(test)]
mod tests {
    include!("reference_graph_tests_01.rs");
}
