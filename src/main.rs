use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::process::{self, Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use console::style;
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use nonempty::nonempty;
use nonempty::NonEmpty;

struct State {
    buf: Vec<Line>,
    pb: ProgressBar,
    max_lines: u16,
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

fn collect(child: &mut Child, sender: &Sender<Line>) -> Result<()> {
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
    child.wait()?;
    t1.join().map_err(|_| anyhow!("thread panicked"))??;
    t2.join().map_err(|_| anyhow!("thread panicked"))??;
    Ok(())
}

fn spawn<F>(cmd: &mut Command, mut process: F) -> Result<()>
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
    t.join().map_err(|_| anyhow!("thread panicked"))??;
    Ok(())
}

fn progress(state: &mut State, line: &Line) -> Result<()> {
    state.buf.push(line.clone());
    let msg = &state.buf[state.buf.len().saturating_sub(state.max_lines as usize)..]
        .iter()
        .map(|line| {
            let msg = style(&line.line).dim();
            format!(
                "{}",
                match line.stream {
                    Stream::Stdout => msg.cyan(),
                    Stream::Stderr => msg.yellow(),
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    state.pb.set_message(format!("\n{}", msg));
    Ok(())
}

pub fn main() -> Result<()> {
    let mut c = build_command(nonempty!["/Users/waltermoreira/repos/athens/cmd.sh"]);
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(200));
    pb.set_style(
        ProgressStyle::with_template("{spinner:.dim.bold} Athens: {wide_msg:.blue}")
            .unwrap()
            .tick_chars("/|\\- "),
    );
    let buf = Vec::new();
    let mut state = State {
        buf,
        pb,
        max_lines: 3,
    };
    spawn(&mut c, |s| progress(&mut state, s))
    //     let started = Instant::now();

    //     println!("Compiling package in release mode...");

    //     let pb = ProgressBar::new_spinner();
    //     pb.enable_steady_tick(Duration::from_millis(200));
    //     pb.set_style(
    //         ProgressStyle::with_template("{spinner:.dim.bold} cargo: {wide_msg}")
    //             .unwrap()
    //             .tick_chars("/|\\- "),
    //     );

    //     let mut p = process::Command::new("sleep")
    //         .arg("5")
    //         .stderr(process::Stdio::piped())
    //         .spawn()
    //         .unwrap();

    //     pb.set_message("\nfoo\nbar\nbaz");
    //     for line in BufReader::new(p.stderr.take().unwrap()).lines() {
    //         let line = line.unwrap();
    //         let stripped_line = line.trim();
    //         if !stripped_line.is_empty() {
    //             pb.set_message(format!("foo\n{}", stripped_line.to_owned()));
    //         }
    //         pb.tick();
    //     }

    //     p.wait().unwrap();

    //     pb.finish_and_clear();

    //     println!("Done in {}", HumanDuration(started.elapsed()));
}

#[cfg(test)]
mod tests {
    use std::{cmp::max, process::Stdio};

    use crate::build_command;
    use anyhow::Result;
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
        Ok(())
    }
}
