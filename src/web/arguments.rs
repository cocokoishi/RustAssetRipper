//! Launch arguments of the product web surface (port of `Arguments.cs`).
//!
//! The frozen baseline parses its command line with Ookii's POSIX parser and then hosts the
//! web interface; there are no subcommands, so a bare executable and a command line made only
//! of options both start the server. The port keeps that entry and reproduces the accepted
//! spellings, the defaults, the single-value duplicate rule, the repeatable
//! `--local-web-file` option and the parser diagnostics. Two port-added rules are marked
//! below: the port's own `serve` word selects the same entry explicitly, and a port outside
//! `u16` is reported as a launch failure instead of failing later at the socket bind.
//!
//! Option values follow Ookii's optional-value shape: a boolean option takes its value only in
//! the attached `--log=false` form, so `--log false` is a positional argument. That behaviour
//! was checked against the shipped `AssetRipper.GUI.Free.exe` probes recorded in
//! `docs/validation-log.md`.

use std::ffi::OsString;

use assetripper::product::{DEFAULT_HEADLESS, DEFAULT_LOG, DEFAULT_PORT};

/// The program name used by the usage and diagnostic text.
pub const PROGRAM_NAME: &str = "assetripper-rs";

/// Parsed launch arguments with the launcher defaults applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchArguments {
    /// `WebApplicationLauncher.Defaults.Port`: zero selects a random unused port.
    pub port: u16,
    /// `WebApplicationLauncher.Defaults.Log`.
    pub log: bool,
    /// `WebApplicationLauncher.Defaults.LogPath`: `None` selects the automatic path.
    pub log_path: Option<String>,
    /// `--local-web-file` values in command-line order.
    pub local_web_files: Vec<String>,
    /// `WebApplicationLauncher.Defaults.Headless`.
    pub headless: bool,
}

impl Default for LaunchArguments {
    fn default() -> Self {
        Self {
            port: u16::try_from(DEFAULT_PORT).unwrap_or(0),
            log: DEFAULT_LOG,
            log_path: None,
            local_web_files: Vec::new(),
            headless: DEFAULT_HEADLESS,
        }
    }
}

/// What a command line means for the launcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchOutcome {
    /// Host the interface with these arguments.
    Launch(Box<LaunchArguments>),
    /// Print the help text and exit without hosting.
    Help,
    /// Print version information and exit without hosting.
    Version,
    /// Print this diagnostic plus the short usage and exit without hosting.
    Failure(String),
}

/// Parse the option words that follow the program name (or the `serve` word).
pub fn parse_launch_arguments<I, T>(arguments: I) -> LaunchOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut parsed = LaunchArguments::default();
    let mut seen: Vec<&'static str> = Vec::new();
    let mut arguments = arguments.into_iter().map(Into::into).peekable();
    while let Some(argument) = arguments.next() {
        let Ok(text) = argument.into_string() else {
            return LaunchOutcome::Failure("the argument text must be valid UTF-8.".to_owned());
        };
        if !text.starts_with('-') {
            return LaunchOutcome::Failure("Too many arguments were supplied.".to_owned());
        }
        let (name, inline) = match text.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (text.clone(), None),
        };
        match name.as_str() {
            "-?" | "-h" | "--help" => {
                if let Some(failure) = record_once(&mut seen, "help") {
                    return failure;
                }
                // The reference validates the optional value and then prints the help text for
                // either boolean, so `--help=false` still shows the help.
                match flag_value("help", inline.as_deref()) {
                    Ok(_) => return LaunchOutcome::Help,
                    Err(failure) => return failure,
                }
            }
            "--version" => {
                if let Some(failure) = record_once(&mut seen, "version") {
                    return failure;
                }
                match flag_value("version", inline.as_deref()) {
                    Ok(_) => return LaunchOutcome::Version,
                    Err(failure) => return failure,
                }
            }
            "--log" => {
                if let Some(failure) = record_once(&mut seen, "log") {
                    return failure;
                }
                match flag_value("log", inline.as_deref()) {
                    Ok(value) => parsed.log = value,
                    Err(failure) => return failure,
                }
            }
            "--headless" => {
                if let Some(failure) = record_once(&mut seen, "headless") {
                    return failure;
                }
                match flag_value("headless", inline.as_deref()) {
                    Ok(value) => parsed.headless = value,
                    Err(failure) => return failure,
                }
            }
            "--port" => {
                if let Some(failure) = record_once(&mut seen, "port") {
                    return failure;
                }
                let value = match required_value("port", inline, &mut arguments) {
                    Ok(value) => value,
                    Err(failure) => return failure,
                };
                let Ok(number) = value.trim().parse::<i32>() else {
                    return LaunchOutcome::Failure(format!(
                        "The value '{value}' provided for argument 'port' could not be interpreted as a 'int32'."
                    ));
                };
                match u16::try_from(number) {
                    Ok(port) => parsed.port = port,
                    Err(_) => {
                        return LaunchOutcome::Failure(format!(
                            "The value '{value}' provided for argument 'port' must be between 0 and 65535."
                        ));
                    }
                }
            }
            "--log-path" => {
                if let Some(failure) = record_once(&mut seen, "log-path") {
                    return failure;
                }
                let value = match required_value("log-path", inline, &mut arguments) {
                    Ok(value) => value,
                    Err(failure) => return failure,
                };
                parsed.log_path = Some(value);
            }
            "--local-web-file" => {
                // Repeatable, so no duplicate check.
                let value = match required_value("local-web-file", inline, &mut arguments) {
                    Ok(value) => value,
                    Err(failure) => return failure,
                };
                parsed.local_web_files.push(value);
            }
            other => {
                return LaunchOutcome::Failure(format!(
                    "Unknown argument name '{}'.",
                    other.trim_start_matches('-')
                ));
            }
        }
    }
    LaunchOutcome::Launch(Box::new(parsed))
}

fn record_once(seen: &mut Vec<&'static str>, name: &'static str) -> Option<LaunchOutcome> {
    if seen.contains(&name) {
        return Some(LaunchOutcome::Failure(format!(
            "The argument '{name}' was supplied more than once."
        )));
    }
    seen.push(name);
    None
}

/// Read an optional boolean value. An absent value is `true`, matching the baseline's
/// `[<boolean>]` usage text.
fn flag_value(name: &str, inline: Option<&str>) -> Result<bool, LaunchOutcome> {
    match inline {
        None => Ok(true),
        Some(text) => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("true") {
                Ok(true)
            } else if trimmed.eq_ignore_ascii_case("false") {
                Ok(false)
            } else {
                Err(LaunchOutcome::Failure(format!(
                    "The value '{text}' provided for argument '{name}' could not be interpreted as a 'boolean'."
                )))
            }
        }
    }
}

fn required_value<T: Into<OsString>>(
    name: &str,
    inline: Option<String>,
    arguments: &mut std::iter::Peekable<impl Iterator<Item = T>>,
) -> Result<String, LaunchOutcome> {
    if let Some(value) = inline {
        return Ok(value);
    }
    let Some(next) = arguments.next() else {
        return Err(LaunchOutcome::Failure(format!(
            "No value was supplied for the argument '{name}'."
        )));
    };
    next.into()
        .into_string()
        .map_err(|_| LaunchOutcome::Failure("the argument text must be valid UTF-8.".to_owned()))
}

/// The one-line usage message, matching the baseline's `Usage:` line shape.
pub fn usage_text() -> String {
    format!(
        "Usage: {PROGRAM_NAME} [--headless] [--help] [--local-web-file <string>...] [--log] [--log-path <string>] [--port <int32>] [--version]"
    )
}

/// The full `--help` text, matching the baseline's option list and descriptions.
pub fn help_text() -> String {
    let mut text = String::new();
    text.push_str(&usage_text());
    text.push_str(
        "\n\n      --headless [<boolean>]\n\
         \x20         If true, a browser window will not be launched automatically. Default value: False.\n\n\
         \x20 -?, --help [<boolean>] (-h)\n\
         \x20         Displays this help message.\n\n\
         \x20     --local-web-file <string>\n\
         \x20         Files provided with this option will replace online sources.\n\n\
         \x20     --log [<boolean>]\n\
         \x20         If true, the application will log to a file. Default value: True.\n\n\
         \x20     --log-path <string>\n\
         \x20         The file location at which to save the log, or a sensible default if not provided.\n\n\
         \x20     --port <int32>\n\
         \x20         If nonzero, the application will attempt to host on this port, instead of finding a random unused port. Default value: 0.\n\n\
         \x20     --version [<boolean>]\n\
         \x20         Displays version information.\n",
    );
    text
}

/// The two lines the baseline's parser prints after a diagnostic.
pub fn short_usage_text() -> String {
    format!(
        "{}\n\nRun '{PROGRAM_NAME} --help' for more information.\n",
        usage_text()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        parse_launch_arguments, short_usage_text, usage_text, LaunchArguments, LaunchOutcome,
    };

    fn launch(arguments: &[&str]) -> LaunchArguments {
        match parse_launch_arguments(arguments.iter().copied()) {
            LaunchOutcome::Launch(parsed) => *parsed,
            other => panic!("expected a launch, got {other:?}"),
        }
    }

    fn failure(arguments: &[&str]) -> String {
        match parse_launch_arguments(arguments.iter().copied()) {
            LaunchOutcome::Failure(message) => message,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn defaults_match_the_launcher() {
        let parsed = launch(&[]);
        assert_eq!(parsed, LaunchArguments::default());
        assert_eq!(parsed.port, 0);
        assert!(parsed.log);
        assert_eq!(parsed.log_path, None);
        assert!(parsed.local_web_files.is_empty());
        assert!(!parsed.headless);
    }

    #[test]
    fn every_option_is_accepted_in_both_value_forms() {
        let parsed = launch(&[
            "--port",
            "5000",
            "--log=false",
            "--headless=true",
            "--log-path",
            "/tmp/assetripper.log",
            "--local-web-file",
            "site.css",
            "--local-web-file=extra.js",
        ]);
        assert_eq!(parsed.port, 5000);
        assert!(!parsed.log);
        assert!(parsed.headless);
        assert_eq!(parsed.log_path.as_deref(), Some("/tmp/assetripper.log"));
        assert_eq!(
            parsed.local_web_files,
            vec!["site.css".to_owned(), "extra.js".to_owned()]
        );
    }

    #[test]
    fn boolean_options_accept_an_optional_attached_value_only() {
        // An absent value is true, mirroring the `[<boolean>]` usage text.
        assert!(launch(&["--log"]).log);
        assert!(launch(&["--headless"]).headless);
        assert!(!launch(&["--log=FALSE"]).log);
        assert!(!launch(&["--headless=False"]).headless);
        // A detached value is a positional argument, exactly as the reference probe showed.
        assert_eq!(
            failure(&["--log", "false"]),
            "Too many arguments were supplied."
        );
        assert_eq!(
            failure(&["--headless", "yes"]),
            "Too many arguments were supplied."
        );
        // A detached value after `--help` is never reached: the help short-circuits first.
        assert_eq!(
            parse_launch_arguments(["--help", "false"]),
            LaunchOutcome::Help
        );
    }

    #[test]
    fn help_and_version_short_circuit_the_launch() {
        for argument in ["--help", "-?", "-h"] {
            assert_eq!(parse_launch_arguments([argument]), LaunchOutcome::Help);
        }
        assert_eq!(
            parse_launch_arguments(["--version"]),
            LaunchOutcome::Version
        );
        // Both booleans print their text; only an unparseable value is a diagnostic.
        assert_eq!(
            parse_launch_arguments(["--help=false", "--port=8080"]),
            LaunchOutcome::Help
        );
        assert_eq!(
            parse_launch_arguments(["--version=false"]),
            LaunchOutcome::Version
        );
        assert_eq!(
            parse_launch_arguments(["--version", "--bogus"]),
            LaunchOutcome::Version
        );
        assert_eq!(
            failure(&["--version=maybe"]),
            "The value 'maybe' provided for argument 'version' could not be interpreted as a 'boolean'."
        );
        assert_eq!(
            failure(&["--help=maybe"]),
            "The value 'maybe' provided for argument 'help' could not be interpreted as a 'boolean'."
        );
    }

    #[test]
    fn diagnostics_match_the_reference_parser() {
        assert_eq!(failure(&["--nope"]), "Unknown argument name 'nope'.");
        assert_eq!(failure(&["-p"]), "Unknown argument name 'p'.");
        // Names are case sensitive in the reference parser.
        assert_eq!(failure(&["--PORT=1"]), "Unknown argument name 'PORT'.");
        assert_eq!(failure(&["foo"]), "Too many arguments were supplied.");
        assert_eq!(
            failure(&["--port", "abc"]),
            "The value 'abc' provided for argument 'port' could not be interpreted as a 'int32'."
        );
        assert_eq!(
            failure(&["--log=maybe"]),
            "The value 'maybe' provided for argument 'log' could not be interpreted as a 'boolean'."
        );
        assert_eq!(
            failure(&["--local-web-file"]),
            "No value was supplied for the argument 'local-web-file'."
        );
        assert_eq!(
            failure(&["--port", "1", "--port", "2"]),
            "The argument 'port' was supplied more than once."
        );
        assert_eq!(
            failure(&["--log", "--log"]),
            "The argument 'log' was supplied more than once."
        );
        // Repeatable options accept several values.
        assert_eq!(
            launch(&["--local-web-file=a", "--local-web-file=b"]).local_web_files,
            vec!["a".to_owned(), "b".to_owned()]
        );
    }

    #[test]
    fn port_range_is_reported_at_parse_time() {
        assert_eq!(launch(&["--port=0"]).port, 0);
        assert_eq!(launch(&["--port=65535"]).port, 65535);
        assert_eq!(
            failure(&["--port=65536"]),
            "The value '65536' provided for argument 'port' must be between 0 and 65535."
        );
        assert_eq!(
            failure(&["--port=-1"]),
            "The value '-1' provided for argument 'port' must be between 0 and 65535."
        );
    }

    #[test]
    fn usage_text_lists_every_option() {
        let usage = usage_text();
        for option in [
            "--headless",
            "--help",
            "--local-web-file",
            "--log",
            "--log-path",
            "--port",
            "--version",
        ] {
            assert!(usage.contains(option), "{option} missing from {usage}");
        }
        assert!(usage.starts_with("Usage: assetripper-rs "));
        let short = short_usage_text();
        assert!(short.contains("Run 'assetripper-rs --help' for more information."));
    }
}
