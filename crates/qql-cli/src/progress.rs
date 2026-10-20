//! TTY-gated progress lines shared by the long-running CLI commands.
//!
//! Progress is one rewritten stderr line. It must never leak into piped
//! output, CI logs, or error text, so every write is gated on `enabled`, and
//! callers derive that flag from [`stderr_is_tty`] plus their own `--quiet` /
//! `--json` switches.

use std::io::{IsTerminal, Write};

/// Whether stderr is a terminal that can host a rewritten progress line.
pub(crate) fn stderr_is_tty() -> bool {
    std::io::stderr().is_terminal()
}

/// A single-line progress renderer over an arbitrary writer. Disabled
/// instances write nothing, so callers need no branch of their own.
pub(crate) struct Progress<W: Write> {
    enabled: bool,
    writer: W,
    statements_total: usize,
    operations_total: usize,
    text: String,
    rendered: bool,
    finished: bool,
}

impl<W: Write> Progress<W> {
    pub(crate) fn new(enabled: bool, writer: W) -> Self {
        Self {
            enabled,
            writer,
            statements_total: 0,
            operations_total: 0,
            text: String::new(),
            rendered: false,
            finished: false,
        }
    }

    pub(crate) fn set_totals(&mut self, statements: usize, operations: usize) {
        self.statements_total = statements;
        self.operations_total = operations;
    }

    pub(crate) fn update(
        &mut self,
        statements_done: usize,
        operations_done: usize,
        succeeded: usize,
        failed: usize,
    ) {
        if !self.enabled || self.finished {
            return;
        }
        self.text = format!(
            "Running {}/{} statements, {}/{} operations ({} ok, {} failed)",
            statements_done,
            self.statements_total,
            operations_done,
            self.operations_total,
            succeeded,
            failed
        );
        self.render();
    }

    /// Clear the line so an error or prompt can print on a clean row.
    pub(crate) fn suspend(&mut self) {
        self.clear();
    }

    /// Re-render the last text after [`Progress::suspend`].
    pub(crate) fn resume(&mut self) {
        if self.enabled && !self.rendered && !self.finished && !self.text.is_empty() {
            self.render();
        }
    }

    /// Clear the line and stop rendering.
    pub(crate) fn finish(&mut self) {
        self.clear();
        self.finished = true;
    }

    fn render(&mut self) {
        let _ = write!(self.writer, "\r\x1b[2K{}", self.text);
        let _ = self.writer.flush();
        self.rendered = true;
    }

    fn clear(&mut self) {
        if self.enabled && self.rendered {
            let _ = write!(self.writer, "\r\x1b[2K");
            let _ = self.writer.flush();
        }
        self.rendered = false;
    }
}

impl<W: Write> Drop for Progress<W> {
    fn drop(&mut self) {
        // `?` inside an execution loop returns without an explicit `finish`;
        // never leave a half-written progress row behind.
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::Progress;

    fn text(buffer: &[u8]) -> String {
        String::from_utf8(buffer.to_vec()).expect("progress output is UTF-8")
    }

    #[test]
    fn disabled_progress_writes_nothing() {
        let mut progress = Progress::new(false, Vec::new());
        progress.set_totals(2, 5);
        progress.update(1, 3, 2, 1);
        progress.suspend();
        progress.resume();
        progress.finish();
        assert!(progress.writer.is_empty());
    }

    #[test]
    fn enabled_progress_rewrites_one_line_and_finish_clears_it() {
        let mut progress = Progress::new(true, Vec::new());
        progress.set_totals(40, 500);
        progress.update(12, 130, 128, 2);

        let rendered = text(&progress.writer);
        assert!(rendered.starts_with("\r\x1b[2K"), "got: {rendered:?}");
        assert!(
            rendered.contains("Running 12/40 statements, 130/500 operations (128 ok, 2 failed)"),
            "got: {rendered:?}"
        );
        assert_eq!(rendered.matches('\r').count(), 1);

        progress.finish();
        let finished = text(&progress.writer);
        assert!(finished.ends_with("\r\x1b[2K"), "got: {finished:?}");
        assert!(
            finished.len() > rendered.len(),
            "finish must clear the rendered line"
        );
    }

    #[test]
    fn suspend_then_resume_rerenders_the_last_text() {
        let mut progress = Progress::new(true, Vec::new());
        progress.set_totals(1, 1);
        progress.update(1, 1, 1, 0);

        progress.suspend();
        let suspended = text(&progress.writer);
        assert!(suspended.ends_with("\r\x1b[2K"), "got: {suspended:?}");

        progress.resume();
        let resumed = text(&progress.writer);
        let line = "Running 1/1 statements, 1/1 operations (1 ok, 0 failed)";
        assert_eq!(resumed.matches(line).count(), 2, "got: {resumed:?}");
        assert!(resumed.ends_with(line), "got: {resumed:?}");
    }

    #[test]
    fn dropping_a_live_progress_line_clears_it() {
        #[derive(Clone, Default)]
        struct Shared(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

        impl std::io::Write for Shared {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.borrow_mut().extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let sink = Shared::default();
        let mut progress = Progress::new(true, sink.clone());
        progress.update(1, 1, 1, 0);
        drop(progress);

        let dropped = text(&sink.0.borrow());
        assert!(dropped.ends_with("\r\x1b[2K"), "got: {dropped:?}");
    }
}
