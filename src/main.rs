use std::cmp::min;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{exit, Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::Parser;
use console::{style, Color, Term};
use indicatif::{ProgressBar, ProgressStyle};
use nonempty::NonEmpty;

const MAX_LINES: u16 = 4;

struct State {
    buf: Vec<Line>,
    pb: ProgressBar,
    max_lines: u16,
    _term_lines: u16,
    term_columns: u16,
}

#[derive(Clone)]
enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone)]
struct Line {
    line: String,
    stream: Stream,
}

impl State {
    fn new() -> Self {
        let term = Term::stdout();
        let (term_lines, term_columns) = term.size();
        let width = (term_columns as usize).saturating_sub(2);
        let width_top = width.saturating_sub(11);
        let top = format!(
            "╭ Running {{spinner:.dim.bold}} {:─<width_top$}╮",
            "",
            width_top = width_top
        );
        let bottom = format!("╰{:─<width$}╯", "", width = width);
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(200));
        pb.set_style(
            ProgressStyle::with_template(&format!("{top}\n{{msg}}\n{bottom}"))
                .expect("error in the ProgressStyle template")
                .tick_chars("/|\\- "),
        );
        Self {
            buf: Default::default(),
            pb,
            max_lines: MAX_LINES,
            _term_lines: term_lines,
            term_columns,
        }
    }

    fn dump(&self) -> Result<PathBuf> {
        let temp = tempfile::NamedTempFile::new()?;
        let (temp, path) = temp.keep()?;
        let mut buf = BufWriter::new(&temp);
        for line in &self.buf {
            writeln!(&mut buf, "{}", line.line)?;
        }
        Ok(path)
    }
}

fn build_command<S>(words: NonEmpty<S>) -> Command
where
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(words.first());
    cmd.args(words.tail());
    cmd
}

fn _read_stream<R>(reader: R, out: &Sender<Line>, stream: Stream) -> Result<()>
where
    R: Read,
{
    let buf = BufReader::new(reader).lines();
    for line in buf {
        let line = line?;
        out.send(Line {
            line,
            stream: stream.clone(),
        })?;
    }
    Ok(())
}

fn collect(child: &mut Child, sender: &Sender<Line>) -> Result<ExitStatus> {
    let err = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("couldn't get stderr"))?;
    let out = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("couldn't get stdout"))?;
    let t1 = thread::spawn({
        let sender = sender.clone();
        move || _read_stream(err, &sender, Stream::Stderr)
    });
    let t2 = thread::spawn({
        let sender = sender.clone();
        move || _read_stream(out, &sender, Stream::Stdout)
    });
    let status = child.wait()?;
    t1.join()
        .map_err(|_| anyhow!("thread panicked while reading stderr"))??;
    t2.join()
        .map_err(|_| anyhow!("thread panicked while reading stdout"))??;
    Ok(status)
}

fn spawn<F>(cmd: &mut Command, mut process: F) -> Result<ExitStatus>
where
    F: FnMut(&Line) -> Result<()>,
{
    let (sender, receiver) = channel();
    cmd.stderr(Stdio::piped()).stdout(Stdio::piped());
    let mut child = cmd.spawn()?;
    let t = thread::spawn(move || collect(&mut child, &sender));
    for x in receiver {
        process(&x)?;
    }
    t.join().map_err(|_| anyhow!("thread panicked"))?
}

fn _draw_line<S>(line: S, width: usize) -> String
where
    S: Display,
{
    format!("│{:<width$}│", line, width = width)
}

// TODO: change to take just State as parameter
fn _build_msg(state: &State) -> String {
    let buf = &state.buf;
    let max_lines = state.max_lines as usize;
    let width = (state.term_columns as usize).saturating_sub(2);
    buf[buf.len().saturating_sub(max_lines)..]
        .iter()
        .map(|line| {
            let l = &line
                .line
                .chars()
                .take(min(line.line.len(), width))
                .collect::<String>();
            let msg = style(l).dim();
            _draw_line(
                match line.stream {
                    Stream::Stdout => msg.cyan(),
                    Stream::Stderr => msg.yellow(),
                },
                width,
            )
        })
        .chain([_draw_line(" ", width)].iter().cloned().cycle())
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n")
}

fn progress(state: &mut State, line: &Line) -> Result<()> {
    state.buf.push(line.clone());
    let msg = _build_msg(state);
    state.pb.set_message(msg);
    Ok(())
}

fn printable_command<S>(command: &NonEmpty<S>) -> OsString
where
    S: AsRef<OsStr>,
{
    command
        .iter()
        .map(|x| x.as_ref())
        .collect::<Vec<_>>()
        .join(&OsString::from(" "))
}

fn spawn_with_progress<S>(command: NonEmpty<S>) -> Result<(ExitStatus, PathBuf)>
where
    S: AsRef<OsStr>,
{
    let mut c = build_command(command);
    let mut state = State::new();
    let initial_msg = _build_msg(&state);
    state.pb.set_message(initial_msg);
    let status = spawn(&mut c, |s| progress(&mut state, s))?;
    state.pb.finish_and_clear();
    let (msg, color) = if status.success() {
        ("Success!".into(), Color::Green)
    } else {
        (
            format!(
                "Command exited with status: {}",
                status
                    .code()
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "none".into())
            ),
            Color::Red,
        )
    };
    let f = state.dump()?;
    println!(
        "{}",
        style(format!("(check full output at: {})", f.to_string_lossy()))
            .fg(color)
    );
    println!("{}", style(msg).fg(color));
    Ok((status, f))
}

#[derive(Parser, Debug)]
#[clap(
    version = "0.1.0",
    author = "Walter Moreira <walter@waltermoreira.net>",
    about = "Run commands using pretty output",
    arg_required_else_help = true
)]
#[clap(propagate_version = true)]
struct Cli {
    #[clap(value_parser, help = "command to run")]
    command: Vec<String>,
    #[clap(short, long, value_parser, help = "Optional name of command")]
    name: Option<OsString>,
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd =
        NonEmpty::from((&cli.command[0], cli.command[1..].iter().collect()));
    let pretty = cli.name.unwrap_or_else(|| printable_command(&cmd));
    println!("Command: {}", pretty.to_string_lossy());
    let (status, _) = spawn_with_progress(cmd)?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| exit(status.code().unwrap_or(1)))
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use indicatif::ProgressBar;

    use crate::{progress, Line, State, Stream, MAX_LINES};

    #[test]
    fn test_unicode_splitting() -> Result<()> {
        let mut state = State {
            buf: Default::default(),
            pb: ProgressBar::new_spinner(),
            max_lines: MAX_LINES,
            _term_lines: 10,
            term_columns: 3,
        };
        let line = Line {
            line: "ëëëëf".into(),
            stream: Stream::Stdout,
        };
        progress(&mut state, &line)?;
        Ok(())
    }
}
