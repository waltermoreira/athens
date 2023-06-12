use std::cmp::min;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::Parser;
use console::{style, Color, Term};
use indicatif::{ProgressBar, ProgressStyle};
use nonempty::{nonempty, NonEmpty};

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
    // Add piping
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

fn progress(state: &mut State, line: &Line) -> Result<()> {
    let width = (state.term_columns as usize).saturating_sub(2);
    state.buf.push(line.clone());
    let msg = &state.buf[state.buf.len().saturating_sub(state.max_lines as usize)..]
        .iter()
        .map(|line| {
            let l = &line.line[..min(line.line.len(), width)];
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
        .take(state.max_lines as usize)
        .collect::<Vec<_>>()
        .join("\n");
    state.pb.set_message(msg.to_string());
    Ok(())
}

fn spawn_with_progress<S>(command: NonEmpty<S>) -> Result<(ExitStatus, PathBuf)>
where
    S: AsRef<OsStr>,
{
    let printable_command = command
        .iter()
        .map(|x| x.as_ref())
        .collect::<Vec<_>>()
        .join(&OsString::from(" "));
    println!("Command: {}", printable_command.to_string_lossy());
    let mut c = build_command(command);
    let mut state = State::new();
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
        style(format!("(check full output at: {})", f.to_string_lossy())).fg(color)
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
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd_and_args = cli.command;
    let head = &cmd_and_args[0];
    let tail = cmd_and_args[1..].iter().collect::<Vec<_>>();
    let cmd = NonEmpty::from((head, tail));
    let (status, _) = spawn_with_progress(cmd)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(""))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::canonicalize, process::Stdio};

    use crate::build_command;
    use anyhow::Result;
    use console::Term;
    use nonempty::nonempty;

    #[test]
    fn test_foo() -> Result<()> {
        let mut c = build_command(nonempty!["/Users/waltermoreira/repos/athens/cmd.sh"]);
        c.stderr(Stdio::piped()).stdout(Stdio::piped());
        dbg!(&c);
        let s = c.output()?;
        dbg!(String::from_utf8(s.stdout)?);
        Ok(())
    }

    #[test]
    fn test_bar() -> Result<()> {
        let v = vec![2, 3, 4];
        let x = v[v.len().saturating_sub(4)..].to_vec();
        dbg!(x);
        let t = Term::stdout();
        dbg!(t.size());
        let x = canonicalize("echo")?;
        dbg!(x);
        Ok(())
    }
}
