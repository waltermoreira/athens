use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::process::{self, Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use nonempty::nonempty;
use nonempty::NonEmpty;

fn build_command<S>(words: NonEmpty<S>) -> Command
where
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(words.first());
    cmd.args(words.tail());
    cmd
}

fn _read_stream<R>(stream: R, out: Sender<String>, tag: &str) -> Result<()>
where
    R: Read,
{
    let buf = BufReader::new(stream).lines();
    for line in buf {
        let the_line = line?;
        out.send(format!("{}: {}", the_line, tag))?;
    }
    Ok(())
}

fn collect(child: &mut Child, sender: &Sender<String>) -> Result<()> {
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
        move || _read_stream(err, sender, "err")
    });
    let t2 = thread::spawn({
        let sender = sender.clone();
        move || _read_stream(out, sender, "out")
    });
    child.wait()?;
    t1.join().map_err(|_| anyhow!("thread panicked"))??;
    t2.join().map_err(|_| anyhow!("thread panicked"))??;
    Ok(())
}

fn spawn<F>(cmd: &mut Command, process: F) -> Result<()>
where
    F: Fn(&str) -> Result<()>,
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

pub fn main() -> Result<()> {
    let mut c = build_command(nonempty!["/Users/waltermoreira/repos/athens/cmd.sh"]);
    spawn(&mut c, |s| Ok(println!("{}", s)))
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
    use std::process::Stdio;

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
}
