//! `twidget-cli`, the command-line front end to twidget: it resolves a widget envelope, lays it
//! out, draws it as SVG, or samples its animations, through the crate's Rust API, so authors can
//! preview a widget without building a client.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use twidget::expr::Env;
use twidget::model::{self, Template};
use twidget::resolve::Instance;
use twidget::{layout, sample, svg, Error};

const USAGE: &str = "\
usage: twidget-cli <resolve|layout|svg|sample> <envelope.json> [options]

  resolve  writes the resolved tree as JSON
  layout   writes the reference layout's frames as JSON
  svg      draws the widget as SVG
  sample   writes each animation's value at --at as JSON

options:
  --templates <dir>  where bundled templates are, as <id>@<version>.json
                     (default fixtures/templates)
  --width <units>    the available width for layout and svg (default 320)
  --now <ms>         the server time in Unix milliseconds (default 1790415687123)
  --tz <minutes>     the UTC offset in minutes (default 210)
  --dark             the dark theme
  --rtl              right to left
  --at <ms>          the server time sample reads, which it needs
  --appear <ms>      when the widget first appeared, for sample (default --now)
  -h, --help         this help";

/// The width `layout` and `svg` lay out in without `--width`, the middle conformance width.
const DEFAULT_WIDTH: f64 = 320.0;

/// Exit code for arguments that do not parse.
const USAGE_ERROR: u8 = 2;

/// Exit code for a widget, file or output that fails.
const FAILURE: u8 = 1;

/// What to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Resolve,
    Layout,
    Svg,
    /// Samples at this server time, for a widget that appeared at the second.
    Sample(i64, i64),
}

/// A command and everything it runs with.
#[derive(Debug)]
struct Run {
    command: Command,
    envelope: PathBuf,
    templates: PathBuf,
    width: f64,
    env: Env,
}

/// Reads a time in whole Unix milliseconds for `option`.
fn milliseconds(text: &str, option: &str) -> Result<i64, String> {
    text.parse()
        .map_err(|_| format!("{option} needs whole milliseconds"))
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match parse(&arguments) {
        Ok(None) => write_out(&format!("{USAGE}\n")),
        Ok(Some(run)) => match output(&run) {
            Ok(text) => write_out(&text),
            Err(message) => fail(&message, FAILURE),
        },
        Err(message) => fail(&format!("{message}\n{USAGE}"), USAGE_ERROR),
    }
}

/// Reads the arguments into a run, or `None` when they ask for help.
fn parse(arguments: &[String]) -> Result<Option<Run>, String> {
    let mut command = None;
    let mut envelope = None;
    let mut templates = PathBuf::from("fixtures/templates");
    let mut width = DEFAULT_WIDTH;
    let mut env = Env::TEST;
    let mut at = None;
    let mut appear = None;
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--templates" => templates = PathBuf::from(value()?),
            "--width" => {
                width = value()?
                    .parse::<f64>()
                    .ok()
                    .filter(|width| width.is_finite() && *width >= 0.0)
                    .ok_or("--width needs a number of units, 0 or more")?;
            }
            "--now" => env.now_ms = milliseconds(value()?, argument)?,
            "--at" => at = Some(milliseconds(value()?, argument)?),
            "--appear" => appear = Some(milliseconds(value()?, argument)?),
            "--tz" => {
                env.tz_offset_minutes = value()?.parse().map_err(|_| "--tz needs whole minutes")?;
            }
            "--dark" => env.dark = true,
            "--rtl" => env.rtl = true,
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            word if command.is_none() => {
                command = Some(match word {
                    "resolve" => Command::Resolve,
                    "layout" => Command::Layout,
                    "svg" => Command::Svg,
                    // The times are known once every option is read.
                    "sample" => Command::Sample(0, 0),
                    other => return Err(format!("unknown command {other}")),
                });
            }
            word if envelope.is_none() => envelope = Some(PathBuf::from(word)),
            word => return Err(format!("unexpected argument {word}")),
        }
    }
    let command = match command.ok_or("missing the command")? {
        Command::Sample(..) => {
            Command::Sample(at.ok_or("sample needs --at")?, appear.unwrap_or(env.now_ms))
        }
        other => other,
    };
    let envelope = envelope.ok_or("missing the envelope file")?;
    Ok(Some(Run {
        command,
        envelope,
        templates,
        width,
        env,
    }))
}

/// What the run writes to standard output, or why it failed.
fn output(run: &Run) -> Result<String, String> {
    let resolved = instance(run)?
        .resolve(&run.env)
        .map_err(|error| described(&error))?;
    let text = match run.command {
        Command::Resolve => format!("{}\n", resolved.json()),
        Command::Layout => {
            let frames = layout::frames(&resolved, run.width, run.env.rtl);
            format!("{}\n", frames.json())
        }
        Command::Svg => svg::render(&resolved, run.width, run.env.rtl, run.env.dark),
        Command::Sample(at_ms, appear_ms) => {
            format!("{}\n", sample::sample(&resolved, at_ms, appear_ms).json())
        }
    };
    Ok(text)
}

/// The envelope bound to its template: the one it carries, or the bundled one it names.
fn instance(run: &Run) -> Result<Instance, String> {
    let envelope = read(&run.envelope)?;
    let bundled = match model::read_template_ref(&envelope) {
        Ok(reference) => {
            let file = format!("{}@{}.json", reference.id, reference.version);
            let path = run.templates.join(file);
            let template = Template::parse(&read(&path)?)
                .map_err(|error| format!("{}: {}", path.display(), described(&error)))?;
            Some(Arc::new(template))
        }
        Err(Error::NotFound) => None,
        Err(error) => return Err(described(&error)),
    };
    Instance::new(&envelope, bundled).map_err(|error| described(&error))
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// A crate failure as the status name the C API reports, then its detail.
fn described(error: &Error) -> String {
    format!("{}: {error}", error.status().name())
}

fn write_out(text: &str) -> ExitCode {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&format!("cannot write the output: {error}"), FAILURE),
    }
}

fn fail(message: &str, code: u8) -> ExitCode {
    // With standard error gone too, the exit code is all that is left to report with.
    let _ = writeln!(io::stderr(), "twidget-cli: {message}");
    ExitCode::from(code)
}
