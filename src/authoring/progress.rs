use std::io::{self, IsTerminal};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};

use super::AuthoringOutcome;
use crate::evidence::InspectionProgress;
use crate::judgment::Judgment;

// {cite}`requirements.authoring.progress`
pub(super) struct BatchProgress {
    bar: Option<ProgressBar>,
    inspection_stage: Option<(&'static str, bool)>,
    verbose: bool,
    positive: usize,
    negative: usize,
    fixme: usize,
    errors: usize,
}

impl BatchProgress {
    // {impl}`requirements.authoring.progress`
    pub(super) fn new(verbose: bool) -> Self {
        let bar = if io::stderr().is_terminal()
            && std::env::var_os("TERM").is_none_or(|term| term != "dumb")
        {
            // Choose terminal support independently of color support: NO_COLOR
            // must leave a readable, monochrome progress display.
            let target = ProgressDrawTarget::term_like_with_hz(
                Box::new(console::Term::buffered_stderr()),
                10,
            );
            let bar = ProgressBar::with_draw_target(None, target).with_style(progress_style(
                "{spinner:.cyan} Inspecting evidence... [{elapsed_precise}]",
            ));
            bar.enable_steady_tick(Duration::from_millis(100));
            Some(bar)
        } else {
            None
        };
        Self {
            bar,
            inspection_stage: None,
            verbose,
            positive: 0,
            negative: 0,
            fixme: 0,
            errors: 0,
        }
    }

    // {impl}`requirements.authoring.progress`
    pub(super) fn inspect(&mut self, update: InspectionProgress) {
        if let Some(bar) = &self.bar {
            let stage = (update.stage, update.total.is_some());
            let changed = self.inspection_stage != Some(stage);
            if changed {
                bar.set_position(0);
                bar.set_style(progress_style(if update.total.is_some() {
                    "{spinner:.cyan} Inspecting evidence [{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent:>3}% {pos}/{len}\n{msg}"
                } else {
                    "{spinner:.cyan} Inspecting evidence [{elapsed_precise}]\n{msg}"
                }));
                bar.set_message(format!("Phase: {}", update.stage));
                self.inspection_stage = Some(stage);
            }
            if let Some(total) = update.total {
                bar.set_length(total as u64);
                bar.set_position(update.completed as u64);
            }
            if changed || update.total == Some(update.completed) {
                bar.force_draw();
            }
        }
    }

    // {impl}`requirements.authoring.progress`
    pub(super) fn start(&self, total: usize) {
        if let Some(bar) = &self.bar {
            bar.set_position(0);
            bar.set_length(total as u64);
            bar.set_style(
                progress_style(
                    "{prefix} {spinner:.cyan} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent:>3}% {pos}/{len} finished\n{msg}",
                )
                .progress_chars("█▉▊▋▌▍▎▏  "),
            );
            bar.set_prefix("Authoring");
            bar.reset_elapsed();
            self.refresh(0, total);
        }
    }

    // {impl}`requirements.authoring.progress`
    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.reporting`
    pub(super) fn completed(&mut self, assessment: &AuthoringOutcome) {
        match &assessment.determination {
            Judgment::True => self.positive += 1,
            Judgment::False => self.negative += 1,
            Judgment::Fixme(_) => self.fixme += 1,
        }
        if matches!(assessment.determination, Judgment::Fixme(_)) {
            self.println(&format!(
                "{}: FIXME (usage: {})",
                assessment.subject,
                assessment.usage_path.display()
            ));
        } else if matches!(assessment.determination, Judgment::False) {
            let mut message = format!(
                "{}: false (usage: {})",
                assessment.subject,
                assessment.usage_path.display()
            );
            if self.verbose {
                message.push('\n');
                message.push_str(&assessment.rationale);
            }
            self.println(&message);
        }
    }

    // {impl}`requirements.authoring.progress`
    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.reporting`
    pub(super) fn failed(&mut self, label: &str, error: &impl std::fmt::Display) {
        self.errors += 1;
        self.println(&format!("{label}: {error}"));
    }

    // {impl}`requirements.authoring.progress`
    pub(super) fn refresh(&self, active: usize, queued: usize) {
        if let Some(bar) = &self.bar {
            bar.set_message(format!(
                "{active} active · {queued} queued | true {} · false {} · FIXME {} · errors {}",
                self.positive, self.negative, self.fixme, self.errors,
            ));
            bar.set_position((self.positive + self.negative + self.fixme + self.errors) as u64);
        }
    }

    // {impl}`requirements.authoring.progress`
    // {impl}`requirements.authoring.batch`
    // {impl}`requirements.authoring.reporting`
    pub(super) fn println(&self, message: &str) {
        if let Some(bar) = &self.bar {
            bar.println(message);
        } else {
            eprintln!("{message}");
        }
    }

    // {impl}`requirements.authoring.progress`
    pub(super) fn finish(&self, interrupted: bool, queued: usize) {
        if let Some(bar) = &self.bar {
            bar.disable_steady_tick();
            self.refresh(0, queued);
            if interrupted {
                bar.set_prefix("Stopped");
                // Finishing normally would move the position to the total,
                // incorrectly counting assessments that were never started.
                bar.abandon();
            } else {
                bar.set_prefix("Finished");
                bar.finish();
            }
            // Indicatif leaves the cursor on the last display line. Keep the
            // CLI's subsequent stdout summary or stderr error on its own line.
            eprintln!();
        }
    }
}

// {impl}`requirements.authoring.progress`
fn progress_style(template: &str) -> ProgressStyle {
    let template = if std::env::var_os("NO_COLOR").is_some() {
        template.replace(":.cyan/blue", "").replace(":.cyan", "")
    } else {
        template.to_owned()
    };
    ProgressStyle::with_template(&template)
        .expect("valid progress template")
        .with_key("percent", write_percentage)
}

// {impl}`requirements.authoring.progress`
fn write_percentage(state: &ProgressState, output: &mut dyn std::fmt::Write) {
    // The library's default rounds 199/200 to 100%. Reserve 100 for completion.
    let total = u128::from(state.len().unwrap_or(1).max(1));
    let percentage = (u128::from(state.pos()) * 100 / total).min(100);
    write!(output, "{percentage}").expect("write progress percentage");
}

impl Drop for BatchProgress {
    // {impl}`requirements.authoring.progress`
    fn drop(&mut self) {
        if let Some(bar) = &self.bar
            && !bar.is_finished()
        {
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    // {test}`requirements.authoring.progress`
    #[derive(Clone, Debug, Default)]
    struct RecordingTerm(Arc<Mutex<String>>);

    impl indicatif::TermLike for RecordingTerm {
        // {test}`requirements.authoring.progress`
        fn width(&self) -> u16 {
            100
        }

        // {test}`requirements.authoring.progress`
        fn move_cursor_up(&self, _lines: usize) -> io::Result<()> {
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn move_cursor_down(&self, _lines: usize) -> io::Result<()> {
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn move_cursor_right(&self, _chars: usize) -> io::Result<()> {
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn move_cursor_left(&self, _chars: usize) -> io::Result<()> {
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn write_line(&self, text: &str) -> io::Result<()> {
            self.0.lock().unwrap().push_str(&format!("{text}\n"));
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn write_str(&self, text: &str) -> io::Result<()> {
            self.0.lock().unwrap().push_str(text);
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn clear_line(&self) -> io::Result<()> {
            Ok(())
        }

        // {test}`requirements.authoring.progress`
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    // {test}`requirements.authoring.progress`
    #[test]
    fn inspection_displays_counts_when_same_phase_discovers_its_total() {
        let terminal = RecordingTerm::default();
        let bar = ProgressBar::with_draw_target(
            None,
            ProgressDrawTarget::term_like(Box::new(terminal.clone())),
        );
        let mut progress = BatchProgress {
            bar: Some(bar),
            inspection_stage: None,
            verbose: false,
            positive: 0,
            negative: 0,
            fixme: 0,
            errors: 0,
        };
        progress.inspect(InspectionProgress {
            stage: "Rust function coverage",
            completed: 0,
            total: None,
        });
        let discovery = std::mem::take(&mut *terminal.0.lock().unwrap());
        assert!(discovery.contains("Phase: Rust function coverage"));
        assert!(!discovery.contains('%'), "{discovery}");

        progress.inspect(InspectionProgress {
            stage: "Rust function coverage",
            completed: 2,
            total: Some(4),
        });
        let counted = std::mem::take(&mut *terminal.0.lock().unwrap());
        assert!(counted.contains("Phase: Rust function coverage"));
        assert!(counted.contains("50% 2/4"), "{counted}");
    }

    // {test}`requirements.authoring.progress`
    #[test]
    fn percentage_does_not_round_unfinished_work_up_to_one_hundred() {
        let bar = ProgressBar::hidden();
        bar.set_length(200);
        for (position, expected) in [(0, "0"), (1, "0"), (199, "99"), (200, "100")] {
            bar.set_position(position);
            let mut output = String::new();
            bar.update(|state| write_percentage(state, &mut output));
            assert_eq!(output, expected);
        }
    }
}
