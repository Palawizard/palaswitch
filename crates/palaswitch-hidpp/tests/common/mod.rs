//! Reading capture files and replaying them through a scripted transport.

// Each test binary uses a different part of this module.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Duration;

use palaswitch_hidpp::{LONG_REPORT_LEN, Transport, TransportError};

/// One line of a capture: what was sent (`TX`) or read back (`RX` on the long
/// collection, `RXS` on the short one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Sent(Vec<u8>),
    Received(Vec<u8>),
}

pub fn parse_capture(capture: &str) -> Vec<Line> {
    capture
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let direction = parts.next().expect("direction");
            let bytes = parts
                .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
                .collect();
            match direction {
                "TX" => Line::Sent(bytes),
                "RX" | "RXS" => Line::Received(bytes),
                other => panic!("unknown direction {other}"),
            }
        })
        .collect()
}

/// Bytes of the line at `index`, whatever its direction.
pub fn line_bytes(capture: &str, index: usize) -> Vec<u8> {
    match parse_capture(capture).swap_remove(index) {
        Line::Sent(bytes) | Line::Received(bytes) => bytes,
    }
}

/// A capture split into exchanges: each sent report with the reports read
/// back after it.
pub fn exchanges(capture: &str) -> Vec<(Vec<u8>, Vec<Vec<u8>>)> {
    let mut exchanges: Vec<(Vec<u8>, Vec<Vec<u8>>)> = Vec::new();
    for line in parse_capture(capture) {
        match line {
            Line::Sent(bytes) => exchanges.push((bytes, Vec::new())),
            Line::Received(bytes) => exchanges.last_mut().expect("TX first").1.push(bytes),
        }
    }
    exchanges
}

/// Transport that checks every write against a script and answers with the
/// reports the script lists after it. A read with nothing queued is a timeout.
#[derive(Debug, Default)]
pub struct ScriptedTransport {
    script: VecDeque<(Vec<u8>, Vec<Vec<u8>>)>,
    inbox: VecDeque<Vec<u8>>,
    pub writes: usize,
}

impl ScriptedTransport {
    pub fn new(script: Vec<(Vec<u8>, Vec<Vec<u8>>)>) -> Self {
        Self {
            script: script.into(),
            ..Self::default()
        }
    }

    /// Reports already waiting before the first write.
    pub fn with_pending(mut self, reports: Vec<Vec<u8>>) -> Self {
        self.inbox.extend(reports);
        self
    }

    pub fn is_done(&self) -> bool {
        self.script.is_empty()
    }
}

impl Transport for ScriptedTransport {
    fn write(&mut self, report: &[u8; LONG_REPORT_LEN]) -> Result<(), TransportError> {
        let (expected, replies) = self.script.pop_front().expect("unexpected write");
        assert_eq!(
            report.as_slice(),
            expected.as_slice(),
            "write {}",
            self.writes
        );
        self.writes += 1;
        self.inbox.extend(replies);
        Ok(())
    }

    fn read(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, TransportError> {
        Ok(self.inbox.pop_front())
    }
}
