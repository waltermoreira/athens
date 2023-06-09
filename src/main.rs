use std::cmp::min;
use std::ffi::OsStr;
use std::fmt::Display;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use console::{style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use nonempty::nonempty;
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
    // Add piping
    cmd.stderr(Stdio::piped()).stdout(Stdio::piped());
    let mut child = cmd.spawn()?;
    let t = thread::spawn(move || collect(&mut child, &sender));
    for x in receiver {
        process(&x)?;
    }
    Ok(t.join().map_err(|_| anyhow!("thread panicked"))??)
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
    let mut c = build_command(command);
    let mut state = State::new();
    let status = spawn(&mut c, |s| progress(&mut state, s))?;
    state.pb.finish_and_clear();
    if status.success() {
        println!("Success!");
    } else {
        println!("Error: {:?}", status.code());
    }
    let f = state.dump()?;
    Ok((status, f))
}

pub fn main() -> Result<()> {
    let x = spawn_with_progress(nonempty!["./cmd.sh"])?;
    dbg!(x);
    Ok(())
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
