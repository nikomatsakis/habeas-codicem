use std::io::{self, IsTerminal};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

// {cite}`habeas:util`
pub(super) struct Progress {
    bar: Option<ProgressBar>,
}

impl Progress {
    // {impl}`requirements.test-coverage.processes.progress.preparation`
    pub(super) fn preparation() -> Self {
        Self::new("Preparing coverage: recording inputs", None)
    }

    // {impl}`requirements.test-coverage.processes.progress`
    pub(super) fn conversion(total: usize) -> Self {
        Self::new("Converting coverage", Some(total))
    }

    // {impl}`requirements.test-coverage.processes.progress.terminal`
    fn new(stage: &'static str, total: Option<usize>) -> Self {
        let target = enabled(
            io::stderr().is_terminal(),
            std::env::var_os("TERM").as_deref(),
        )
        .then(|| {
            // Use terminal support independently of color support. The
            // monochrome display also works when NO_COLOR is set.
            ProgressDrawTarget::term_like_with_hz(Box::new(console::Term::buffered_stderr()), 10)
        });
        Self::with_target(stage, total, target)
    }

    // {cite}`habeas:util`
    fn with_target(
        stage: &'static str,
        total: Option<usize>,
        target: Option<ProgressDrawTarget>,
    ) -> Self {
        let bar = target.map(|target| {
            let template = if total.is_some() {
                "{spinner} {msg} [{elapsed_precise}] [{wide_bar}] {pos}/{len} attempts"
            } else {
                "{spinner} {msg} [{elapsed_precise}]"
            };
            let bar = ProgressBar::with_draw_target(total.map(|total| total as u64), target)
                .with_style(
                    ProgressStyle::with_template(template).expect("valid progress template"),
                );
            bar.set_message(stage);
            bar.enable_steady_tick(Duration::from_millis(100));
            bar.force_draw();
            bar
        });
        Self { bar }
    }

    // {cite}`habeas:util`
    pub(super) fn stage(&self, stage: &'static str) {
        if let Some(bar) = &self.bar {
            bar.set_message(stage);
            bar.force_draw();
        }
    }

    // {cite}`habeas:util`
    pub(super) fn completed(&self) {
        if let Some(bar) = &self.bar {
            bar.inc(1);
            if bar.length() == Some(bar.position()) {
                bar.force_draw();
            }
        }
    }
}

// {impl}`requirements.test-coverage.processes.progress.terminal`
fn enabled(terminal: bool, term: Option<&std::ffi::OsStr>) -> bool {
    terminal && term.is_none_or(|term| term != "dumb")
}

impl Drop for Progress {
    // {impl}`requirements.test-coverage.processes.progress.terminal`
    fn drop(&mut self) {
        if let Some(bar) = &self.bar {
            bar.disable_steady_tick();
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Debug, Default)]
    struct RecordingTerm {
        text: Arc<Mutex<String>>,
        frames: Arc<Mutex<Vec<String>>>,
        clears: Arc<Mutex<usize>>,
    }

    impl indicatif::TermLike for RecordingTerm {
        fn width(&self) -> u16 {
            100
        }
        fn move_cursor_up(&self, _lines: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_cursor_down(&self, _lines: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_cursor_right(&self, _chars: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_cursor_left(&self, _chars: usize) -> io::Result<()> {
            Ok(())
        }
        fn write_line(&self, text: &str) -> io::Result<()> {
            self.text.lock().unwrap().push_str(&format!("{text}\n"));
            self.frames.lock().unwrap().push(text.to_owned());
            Ok(())
        }
        fn write_str(&self, text: &str) -> io::Result<()> {
            self.text.lock().unwrap().push_str(text);
            self.frames.lock().unwrap().push(text.to_owned());
            Ok(())
        }
        fn clear_line(&self) -> io::Result<()> {
            *self.clears.lock().unwrap() += 1;
            Ok(())
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    // {test}`requirements.test-coverage.processes.progress`
    // {test}`requirements.test-coverage.processes.progress.preparation`
    // {test}`requirements.test-coverage.processes.progress.terminal`
    #[test]
    fn stage_and_conversion_counts_are_visible_then_cleared() {
        let terminal = RecordingTerm::default();
        let target = || Some(ProgressDrawTarget::term_like(Box::new(terminal.clone())));
        {
            let preparation = Progress::with_target("Recording inputs", None, target());
            assert!(terminal.text.lock().unwrap().contains("Recording inputs"));
            preparation.stage("Capturing coverage maps");
            assert!(
                terminal
                    .text
                    .lock()
                    .unwrap()
                    .contains("Capturing coverage maps")
            );
            let before_drop = *terminal.clears.lock().unwrap();
            drop(preparation);
            assert!(*terminal.clears.lock().unwrap() > before_drop);
        }
        let cleared = *terminal.clears.lock().unwrap();
        assert!(cleared > 0);
        for total in [0, 2] {
            let conversion = Progress::with_target("Converting coverage", Some(total), target());
            let bar = conversion.bar.as_ref().unwrap();
            assert_eq!(bar.position(), 0);
            for expected in 1..=total {
                conversion.completed();
                bar.force_draw();
                assert_eq!(bar.position(), expected as u64);
                let text = terminal.text.lock().unwrap();
                assert!(
                    text.contains(&format!("{expected}/{total} attempts")),
                    "{text}"
                );
                assert!(text.contains("[00:00:"));
                assert!(!text.contains("\x1b["), "display must be monochrome");
            }
            let before_drop = *terminal.clears.lock().unwrap();
            drop(conversion);
            assert!(
                *terminal.clears.lock().unwrap() > before_drop,
                "completed and empty displays must both clear on drop"
            );
        }
        assert!(*terminal.clears.lock().unwrap() > cleared);
    }

    // {test}`requirements.test-coverage.processes.progress.preparation`
    #[test]
    fn preparation_activity_keeps_updating_without_a_stage_change() {
        let terminal = RecordingTerm::default();
        let _preparation = Progress::with_target(
            "Recording inputs",
            None,
            Some(ProgressDrawTarget::term_like(Box::new(terminal.clone()))),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let frames = terminal
                .frames
                .lock()
                .unwrap()
                .iter()
                .filter(|frame| frame.contains("Recording inputs"))
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if frames.len() >= 2 {
                assert!(frames.iter().all(|frame| frame.contains("[00:00:")));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "preparation activity did not update: {frames:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // {test}`requirements.test-coverage.processes.progress.terminal`
    #[test]
    fn terminal_policy_and_hidden_progress_emit_no_display() {
        for (terminal, term, expected) in [
            (true, None, true),
            (true, Some("xterm"), true),
            (true, Some("dumb"), false),
            (false, None, false),
            (false, Some("xterm"), false),
        ] {
            assert_eq!(enabled(terminal, term.map(std::ffi::OsStr::new)), expected);
        }
        let hidden = Progress::with_target("Converting coverage", Some(1), None);
        hidden.completed();
        hidden.stage("No display");
        assert!(hidden.bar.is_none());
    }
}
