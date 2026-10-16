//! The product web entry: print the banner, log, bind, serve and open a browser.
//!
//! This mirrors `WebApplicationLauncher.Launch`: the ASCII banner and the "Now listening on:"
//! line the banner points at, file logging next to the executable with the five-newest
//! rotation, the system-information block, the browser launch that `--headless` suppresses, and
//! one request per connection.
//!
//! Differences that are recorded in `docs/feature-parity.md`:
//!
//! * The port renders timestamps in UTC (the baseline uses the local clock for the log file
//!   name and UTC for the export subfolder). Only the text of a generated name changes.
//! * The system-information block reports the port's own identity and the values the standard
//!   library exposes; the baseline reports its assembly and runtime build metadata.
//! * `HEAD` answers with the same status and headers as `GET` and no body.
//!
//! Requests are answered on the connection that carried them, then the connection closes
//! (`Connection: close`), which is what keeps the server free of any async runtime.

use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use assetripper::product::{
    automatic_log_file_name, automatic_log_path_selected, select_rotated_log_deletions,
    ROTATED_LOG_FILE_PATTERN,
};

use super::arguments::LaunchArguments;
use super::http::{error_response, Request};
use super::routes;
use super::state::WebState;

/// The banner `WelcomeMessage.Print` shows before serving.
const BANNER: &str = r#"
                       _   _____  _                       
    /\                | | |  __ \(_)                      
   /  \   ___ ___  ___| |_| |__) |_ _ __  _ __   ___ _ __ 
  / /\ \ / __/ __|/ _ \ __|  _  /| | '_ \| '_ \ / _ \ '__|
 / ____ \\__ \__ \  __/ |_| | \ \| | |_) | |_) |  __/ |   
/_/    \_\___/___/\___|\__|_|  \_\_| .__/| .__/ \___|_|   
                                   | |   | |              
                                   |_|   |_|              
"#;

/// The directions the banner is followed by.
const DIRECTIONS: &str = r#"In a moment, a line will appear: "Now listening on:" followed by a url.
Open that url in any web browser to access the AssetRipper user interface."#;

/// Serve the browser interface until the process is stopped.
pub fn launch(arguments: &LaunchArguments) -> Result<(), Box<dyn Error>> {
    // The baseline collects local web files before it prints the banner, so a missing file is
    // reported first.
    let static_content = collect_static_content(&arguments.local_web_files);
    print!("{BANNER}");
    println!();
    println!("{DIRECTIONS}");
    println!();

    let mut log = SessionLog::open(arguments)?;
    log.system_information();

    let mut state = WebState::new(arguments.headless)?;
    state.set_static_content(static_content);

    let listener = TcpListener::bind(("127.0.0.1", arguments.port))?;
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}");
    log.line(&format!("Now listening on: {url}"));
    log.line("Application started. Press Ctrl+C to shut down.");
    if !arguments.headless {
        open_browser(&url, &mut log);
    }
    serve(&listener, &mut state, &mut log)
}

/// Accept connections one at a time and answer each request.
fn serve(
    listener: &TcpListener,
    state: &mut WebState,
    log: &mut SessionLog,
) -> Result<(), Box<dyn Error>> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, state, log) {
                    log.line(&format!("The request failed: {error}"));
                }
            }
            Err(error) => log.line(&format!("The connection failed: {error}")),
        }
    }
    Ok(())
}

/// Read one request, dispatch it and write the response.
fn handle_connection(
    stream: TcpStream,
    state: &mut WebState,
    log: &mut SessionLog,
) -> Result<(), Box<dyn Error>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let request = match Request::read(&mut reader) {
        Ok(Some(request)) => request,
        // The peer closed the connection without sending a request.
        Ok(None) => return Ok(()),
        Err(error) => {
            let mut writer = stream;
            error_response(&error).write_to(&mut writer)?;
            return Ok(());
        }
    };
    let head_only = request.method() == "HEAD";
    let response = routes::dispatch(state, &request);
    log.line(&format!(
        "Request {method} {target} -> {status} {content_type} ({bytes} bytes)",
        method = request.method(),
        target = request.target(),
        status = response.status(),
        content_type = response.content_type(),
        bytes = response.body().len(),
    ));
    let mut writer = stream;
    if head_only {
        response.write_head(&mut writer)?;
    } else {
        response.write_to(&mut writer)?;
    }
    for line in state.take_log() {
        log.line(&line);
    }
    Ok(())
}

/// Read the `--local-web-file` values into static content entries.
///
/// The baseline maps the file extension to a URL prefix and reports a missing file instead of
/// failing, which is what this does.
fn collect_static_content(paths: &[String]) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        match fs::read(path) {
            Ok(bytes) => {
                let name = Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let prefix = match Path::new(&name).extension().and_then(|it| it.to_str()) {
                    Some("css") => "/css/",
                    Some("js") => "/js/",
                    _ => "/",
                };
                entries.push((format!("{prefix}{name}"), bytes));
            }
            Err(_) => println!("File '{path}' does not exist."),
        }
    }
    entries
}

/// Open the interface in the default browser, reporting a failure instead of aborting.
fn open_browser(url: &str, log: &mut SessionLog) {
    let mut command = match std::env::consts::OS {
        "windows" => {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "start", ""]);
            command
        }
        "macos" => std::process::Command::new("open"),
        _ => std::process::Command::new("xdg-open"),
    };
    match command.arg(url).spawn() {
        Ok(_) => {}
        Err(error) => log.line(&format!("Failed to open the browser: {error}")),
    }
}

/// The port's log sink: the console always, plus the rotating file when logging is on.
struct SessionLog {
    file: Option<File>,
}

impl SessionLog {
    /// Open the log file the launcher selects, rotating the automatic path first.
    fn open(arguments: &LaunchArguments) -> Result<Self, Box<dyn Error>> {
        if !arguments.log {
            return Ok(Self { file: None });
        }
        let requested = arguments
            .log_path
            .as_deref()
            .filter(|value| !value.is_empty());
        let path = match requested {
            Some(path) => PathBuf::from(path),
            None => {
                let directory = crate::export_commands::executable_directory()?;
                if automatic_log_path_selected(arguments.log_path.as_deref()) {
                    rotate_logs(&directory);
                }
                directory.join(automatic_log_file_name(&utc_timestamp_text(
                    SystemTime::now(),
                )))
            }
        };
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { file: Some(file) })
    }

    /// The block `Logger.LogSystemInformation` writes.
    fn system_information(&mut self) {
        self.line("AssetRipper");
        self.line(&format!(
            "Operating System: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        self.line(&format!("AssetRipper Version: {}", super::PRODUCT_VERSION));
        self.line("AssetRipper Build Type: Release");
        self.line(&format!(
            "UTC Current Time: {}",
            utc_timestamp_text(SystemTime::now())
        ));
    }

    /// Write one line to the console and the log file.
    fn line(&mut self, text: &str) {
        println!("{text}");
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{text}");
        }
    }
}

/// Delete the oldest automatic log files so at most the baseline's budget remains.
///
/// Rotation runs before the new file is created, exactly like `RotateLogs`.
fn rotate_logs(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches_log_pattern(&name) {
            names.push(name);
        }
    }
    names.sort();
    for name in select_rotated_log_deletions(&names) {
        let _ = fs::remove_file(directory.join(name));
    }
}

/// Whether a file name matches the rotation pattern (`AssetRipper_*.log`).
fn matches_log_pattern(name: &str) -> bool {
    match ROTATED_LOG_FILE_PATTERN.split_once('*') {
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
        None => name == ROTATED_LOG_FILE_PATTERN,
    }
}

/// Render the `yyyyMMdd_HHmmss` timestamp text the launcher uses for names.
pub fn utc_timestamp_text(now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let second_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}_{hour:02}{minute:02}{second:02}",
        hour = second_of_day / 3_600,
        minute = (second_of_day / 60) % 60,
        second = second_of_day % 60,
    )
}

/// Convert days since the Unix epoch to a civil date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, matches_log_pattern, utc_timestamp_text};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn the_timestamp_text_matches_the_name_pattern() {
        assert_eq!(utc_timestamp_text(UNIX_EPOCH), "19700101_000000");
        let midnight = UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        assert_eq!(utc_timestamp_text(midnight), "20260101_000000");
        let noonish = midnight + Duration::from_secs(12 * 3_600 + 34 * 60 + 56);
        assert_eq!(utc_timestamp_text(noonish), "20260101_123456");
        // A leap day and a month boundary.
        assert_eq!(
            utc_timestamp_text(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "20000229_000000"
        );
        assert_eq!(
            utc_timestamp_text(UNIX_EPOCH + Duration::from_secs(1_735_689_600)),
            "20250101_000000"
        );
    }

    #[test]
    fn the_civil_conversion_handles_the_epoch_boundary() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn the_log_pattern_matches_only_automatic_log_names() {
        assert!(matches_log_pattern("AssetRipper_20260101_000000.log"));
        assert!(!matches_log_pattern("AssetRipper.log"));
        assert!(!matches_log_pattern("AssetRipper_20260101_000000.txt"));
        assert!(!matches_log_pattern("other_20260101_000000.log"));
    }
}
