//! Source-compatible detection and recursive decoding of top-level file schemes.

include!("scheme_models_and_errors.rs");
include!("scheme_container_mount.rs");
include!("scheme_reader_dispatch.rs");

#[cfg(test)]
mod tests {
    include!("scheme_tests_01.rs");
    include!("scheme_tests_02.rs");
}
