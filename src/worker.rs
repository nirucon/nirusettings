//! The worker thread: everything that may take longer than a frame (a
//! checker, a write and its apply command, asking niructl and nirux) runs
//! here, and the window is woken when an answer is ready.
use crate::gui::shell::Waker;
use nirusettings::sources::{self, Extra, Key, Nirux};
use nirusettings::writer::{Done, Job, Writer};
use std::sync::mpsc::{channel, Receiver, Sender};

/// What to refresh when a terminal command has finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Extras,
    Nirux,
}

pub enum Req {
    Write(Job),
    Keys(toml::Value),
    Extras,
    Nirux,
    Versions(Vec<String>),
    Terminal { argv: Vec<String>, after: After },
}

pub enum Resp {
    Written(Done),
    Keys(Vec<Key>, String),
    Extras(Vec<Extra>, String),
    Nirux(Nirux),
    Versions(Vec<(String, Option<String>)>),
    TerminalDone { after: After, error: Option<String> },
}

pub struct Worker {
    pub tx: Sender<Req>,
    pub rx: Receiver<Resp>,
}

pub fn start(waker: Waker) -> Worker {
    let (tx, jobs) = channel::<Req>();
    let (out, rx) = channel::<Resp>();
    std::thread::spawn(move || {
        let mut writer = Writer::default();
        for req in jobs {
            let resp = match req {
                Req::Write(job) => Resp::Written(writer.run(&job)),
                Req::Keys(cfg) => {
                    let (k, from) = sources::keys(&cfg);
                    Resp::Keys(k, from)
                }
                Req::Extras => {
                    let (e, from) = sources::extras();
                    Resp::Extras(e, from)
                }
                Req::Nirux => Resp::Nirux(sources::nirux()),
                Req::Versions(bins) => Resp::Versions(
                    bins.into_iter()
                        .map(|b| {
                            let v = sources::version(&b);
                            (b, v)
                        })
                        .collect(),
                ),
                Req::Terminal { argv, after } => {
                    // Waited for in a thread of its own: the terminal stays
                    // open as long as the person reads it.
                    let out = out.clone();
                    std::thread::spawn(move || {
                        let error = match std::process::Command::new(&argv[0])
                            .args(&argv[1..])
                            .stdin(std::process::Stdio::null())
                            .status()
                        {
                            Ok(_) => None,
                            Err(e) => Some(format!("cannot start {}: {e}", argv[0])),
                        };
                        let _ = out.send(Resp::TerminalDone { after, error });
                        waker.wake();
                    });
                    continue;
                }
            };
            if out.send(resp).is_err() {
                break;
            }
            waker.wake();
        }
    });
    Worker { tx, rx }
}
