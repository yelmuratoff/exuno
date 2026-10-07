//! The engine's log voice, descended from `lib/helpers/logging.sh`. `check`
//! captures the render log plain, the way `lib/check.sh` captured `sync.sh`
//! into a file where `_use_colors` is false; `sync` streams it, coloured on a
//! terminal. Every line for a human reader goes to stderr; stdout carries
//! only what a program reads, `--help` and `--json`
//! (`.ai/src/rules/cli-output.md`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// Receives each line as it is written, without its newline.
pub type Sink = Box<dyn FnMut(Stream, &str)>;

#[derive(Default)]
pub struct Log {
    lines: Vec<(Stream, String)>,
    sink: Option<Sink>,
    colors: bool,
    quiet: bool,
}

const RESET: &str = "\x1b[0m";
const BLUE: &str = "\x1b[0;34m";
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[0;33m";
const RED: &str = "\x1b[0;31m";
const CYAN: &str = "\x1b[0;36m";

impl Log {
    /// A log that keeps its lines for `lines()`, coloured when `colors`.
    pub fn capturing(colors: bool) -> Self {
        Self {
            lines: Vec::new(),
            sink: None,
            colors,
            quiet: false,
        }
    }

    /// A log that hands every line to `sink` instead of keeping it; `colors` is
    /// `_use_colors`, decided by the caller from stderr and `NO_COLOR`.
    pub fn streaming(colors: bool, sink: Sink) -> Self {
        Self {
            lines: Vec::new(),
            sink: Some(sink),
            colors,
            quiet: false,
        }
    }

    /// `--quiet`: keep warnings, errors, and the closing `[DONE]` line; drop
    /// the progress in between.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// A level tag, coloured on a terminal and plain otherwise. No glyph: the
    /// tag names the level in words, so nothing depends on a font having the
    /// character or on a terminal agreeing how wide it is.
    fn tagged(&mut self, stream: Stream, color: &str, tag: &str, msg: &str) {
        let line = if self.colors {
            format!("{color}{tag}{RESET} {msg}")
        } else {
            format!("{tag} {msg}")
        };
        self.emit(stream, line);
    }

    /// A command the reader should run, coloured as `Style::cyan` colours it
    /// in a command's report, so it stands out from the prose around it.
    pub fn command(&self, cmd: &str) -> String {
        if self.colors {
            format!("{CYAN}{cmd}{RESET}")
        } else {
            cmd.to_string()
        }
    }

    pub fn info(&mut self, msg: &str) {
        if self.quiet {
            return;
        }
        self.tagged(Stream::Err, BLUE, "[INFO]", msg);
    }

    pub fn warning(&mut self, msg: &str) {
        self.tagged(Stream::Err, YELLOW, "[WARNING]", msg);
    }

    pub fn error(&mut self, msg: &str) {
        self.tagged(Stream::Err, RED, "[ERROR]", msg);
    }

    pub fn done(&mut self, msg: &str) {
        self.tagged(Stream::Err, GREEN, "[DONE]", msg);
    }

    pub fn step(&mut self, msg: &str) {
        if self.quiet {
            return;
        }
        self.emit(Stream::Err, format!("   {msg}"));
    }

    /// The empty line that ends a block.
    pub fn blank(&mut self) {
        if self.quiet {
            return;
        }
        self.emit(Stream::Err, String::new());
    }

    /// A line for a program to read: help text, or the `--json` summary.
    pub fn out(&mut self, line: String) {
        self.emit(Stream::Out, line);
    }

    pub fn err(&mut self, line: String) {
        self.emit(Stream::Err, line);
    }

    fn emit(&mut self, stream: Stream, line: String) {
        match &mut self.sink {
            Some(sink) => sink(stream, &line),
            None => self.lines.push((stream, line)),
        }
    }

    pub fn lines(&self) -> &[(Stream, String)] {
        &self.lines
    }

    /// The last `n` lines of both streams in the order they were written, as
    /// `tail -n` shows a `>file 2>&1` capture.
    pub fn tail(&self, n: usize) -> Vec<&str> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines[skip..]
            .iter()
            .map(|(_, line)| line.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn plain_prefixes_match_logging_sh_without_a_terminal() {
        let mut log = Log::default();
        log.info("a");
        log.warning("b");
        log.error("c");
        log.step("d");
        log.done("f");
        let lines: Vec<&str> = log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            ["[INFO] a", "[WARNING] b", "[ERROR] c", "   d", "[DONE] f"]
        );
        assert!(log.lines().iter().all(|(s, _)| *s == Stream::Err));
    }

    #[test]
    fn only_out_reaches_stdout() {
        let mut log = Log::default();
        log.out("{}".to_string());
        log.blank();
        log.info("a");
        assert_eq!(
            log.lines(),
            [
                (Stream::Out, "{}".to_string()),
                (Stream::Err, String::new()),
                (Stream::Err, "[INFO] a".to_string())
            ]
        );
    }

    #[test]
    fn quiet_keeps_warnings_errors_done_and_stdout_only() {
        let mut log = Log::default();
        log.set_quiet(true);
        log.info("a");
        log.step("b");
        log.blank();
        log.warning("c");
        log.error("d");
        log.done("e");
        log.out("f".to_string());
        let lines: Vec<&str> = log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(lines, ["[WARNING] c", "[ERROR] d", "[DONE] e", "f"]);
    }

    #[test]
    fn tail_keeps_the_last_lines_in_write_order() {
        let mut log = Log::default();
        for i in 0..5 {
            log.out(i.to_string());
        }
        assert_eq!(log.tail(2), ["3", "4"]);
        assert_eq!(log.tail(40).len(), 5);
    }

    #[test]
    fn a_streaming_log_hands_coloured_lines_to_its_sink_in_order() {
        let seen: Rc<RefCell<Vec<(Stream, String)>>> = Rc::default();
        let sink_seen = Rc::clone(&seen);
        let mut log = Log::streaming(
            true,
            Box::new(move |stream, line| sink_seen.borrow_mut().push((stream, line.to_string()))),
        );
        log.info("Syncing Claude Code");
        log.warning("w");
        log.error("e");
        log.done("Synced 1/1 tools");
        log.step("s");
        assert!(log.lines().is_empty());
        assert_eq!(
            *seen.borrow(),
            [
                (
                    Stream::Err,
                    "\x1b[0;34m[INFO]\x1b[0m Syncing Claude Code".to_string()
                ),
                (Stream::Err, "\x1b[0;33m[WARNING]\x1b[0m w".to_string()),
                (Stream::Err, "\x1b[0;31m[ERROR]\x1b[0m e".to_string()),
                (
                    Stream::Err,
                    "\x1b[0;32m[DONE]\x1b[0m Synced 1/1 tools".to_string()
                ),
                (Stream::Err, "   s".to_string()),
            ]
        );
    }

    #[test]
    fn a_command_is_cyan_only_when_the_log_colours() {
        assert_eq!(
            Log::capturing(true).command("exuno sync"),
            "\x1b[0;36mexuno sync\x1b[0m"
        );
        assert_eq!(Log::default().command("exuno sync"), "exuno sync");
    }
}
