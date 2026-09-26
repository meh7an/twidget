//! `twidget-cli`, the command-line front end to twidget.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("usage: twidget-cli <command> [options]");
    eprintln!("this build has no commands");
    ExitCode::FAILURE
}
