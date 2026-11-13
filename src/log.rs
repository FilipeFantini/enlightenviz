//! One line of text to standard error per event, as `key=value` pairs. A
//! container's log collector adds the time. Never given a connection string.

use std::io::Write;

pub fn info(message: &str) {
    write_line(format_args!("level=info msg={message:?}"));
}

/// A line tied to the id that was also sent to the client, so the two can be
/// matched.
pub fn error(id: &str, message: &str) {
    write_line(format_args!("level=error id={id} msg={message:?}"));
}

fn write_line(line: std::fmt::Arguments<'_>) {
    // If standard error is gone there is nowhere left to report that.
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}
