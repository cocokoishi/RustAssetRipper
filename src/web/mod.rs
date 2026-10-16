//! The product's browser interface: a small, dependency-free HTTP server that serves the
//! pages, commands and probes of the frozen `AssetRipper.GUI.Web` application.
//!
//! The entry point is [`run`]: the baseline binary has no subcommands, so a bare invocation
//! starts this server, and the port's `serve` word selects it explicitly. The launch arguments,
//! their defaults and their diagnostics are ported from `Arguments.cs`; see
//! [`arguments`] for the two port-added rules.
//!
//! Module layout:
//!
//! * [`arguments`] - the launch-argument parser and its help/usage text.
//! * [`http`] - bounded request parsing and response writing.
//! * [`html`] - an HTML escape and element builder.
//! * [`localization`] - the vendored message tables and their English fallback.
//! * [`settings_form`] - the settings form model and the `POST /Settings/Update` semantics.
//! * [`pages`] - one renderer per baseline page.
//! * [`routes`] - request dispatch over the baseline route table.
//!
//! The bundle-tree path values and their view URLs live in the library as
//! [`assetripper::web_paths`], beside the shared JSON reader they are built on.
//! * [`server`] - binding, logging, the banner and the connection loop.
//! * [`state`] - the loaded game, the settings record and the log tail.

pub mod arguments;
pub mod html;
pub mod http;
pub mod localization;
pub mod pages;
pub mod routes;
pub mod server;
pub mod settings_form;
pub mod state;

use std::error::Error;
use std::ffi::OsString;

use arguments::{help_text, parse_launch_arguments, short_usage_text, LaunchOutcome};

/// The port's product version, reported in the startup log.
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The version text `serve --version` prints.
///
/// The first two lines are the reference program's own text (the port keeps the frozen
/// baseline's version); the third line names the implementation that is running.
const VERSION_TEXT: &str = concat!(
    "AssetRipper.GUI.Web 2.0.0\n",
    "Copyright \u{a9} ds5678\n",
    "assetripper-rs: an independent Rust implementation of AssetRipper",
);

/// Serve the interface, or print the requested help/version text.
pub fn run(arguments: &[OsString]) -> Result<(), Box<dyn Error>> {
    match parse_launch_arguments(arguments.iter().cloned()) {
        LaunchOutcome::Launch(parsed) => server::launch(&parsed),
        LaunchOutcome::Help => {
            print!("{}", help_text());
            Ok(())
        }
        LaunchOutcome::Version => {
            println!("{VERSION_TEXT}");
            Ok(())
        }
        // The baseline prints the parser diagnostic, the short usage and exits successfully.
        LaunchOutcome::Failure(diagnostic) => {
            println!("{diagnostic}");
            println!();
            print!("{}", short_usage_text());
            println!("Run 'assetripper-rs --help' for more information.");
            Ok(())
        }
    }
}
