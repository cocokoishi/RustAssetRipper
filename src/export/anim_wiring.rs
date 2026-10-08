//! Lane integration surface for the animation processing and export workstream.
//!
//! This module exists so that workstream can wire clip/controller/avatar processing into the
//! product path without editing `src/main.rs`, `src/export/project_workflow.rs`, or another
//! lane's files. It starts empty on purpose: a lane adds code here only together with the
//! tests and the evidence that the wired behavior is complete.
