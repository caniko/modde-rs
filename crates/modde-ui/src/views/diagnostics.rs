use crate::views::selectable_text::{TextStyle, danger, primary, success, text, warning};
use iced::widget::{button, column, container, row, scrollable, text_input};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;

/// A single diagnostic finding.
#[derive(Debug, Clone)]
pub struct DiagnosticEntry {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

/// Staged profile integrity results included in the diagnostics report.
#[derive(Debug, Clone, Default)]
pub struct IntegritySummary {
    pub ok_count: usize,
    pub broken_symlinks: Vec<PathBuf>,
}

/// Combined diagnostics and integrity report for the active profile.
#[derive(Debug, Clone)]
pub struct DiagnosticsReport {
    pub profile_name: String,
    pub game_id: String,
    pub entries: Vec<DiagnosticEntry>,
    pub integrity: IntegritySummary,
    pub crash_report: Option<modde_core::crash::CrashCorrelationReport>,
}

/// Severity levels for diagnostics.
#[derive(Debug, Clone)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// State machine for the diagnostics view.
#[derive(Debug, Clone, Default)]
pub enum DiagnosticsState {
    #[default]
    Idle,
    Running,
    Error(String),
    Complete(DiagnosticsReport),
}

/// Render the diagnostics view.
pub fn view<'a>(state: &'a DiagnosticsState, crash_log_path: &'a str) -> Element<'a, Message> {
    let running = matches!(state, DiagnosticsState::Running);

    let title_bar = row![
        text("Diagnostics").size(20),
        iced::widget::space::horizontal(),
        button(
            text(if running {
                "Running..."
            } else {
                "Run Diagnostics"
            })
            .size(14)
        )
        .style(button::primary)
        .padding([6, 14])
        .on_action_maybe(
            (!running).then_some(ButtonAction::RunDiagnostics),
            "Diagnostics are already running.",
        ),
    ]
    .align_y(Alignment::Center);

    let crash_controls = row![
        text_input("Crash log path", crash_log_path)
            .on_input(Message::CrashLogPathChanged)
            .on_submit(Message::AnalyzeCrashLog)
            .width(Length::Fill),
        button(text("Analyze Crash Log").size(14))
            .style(button::secondary)
            .padding([6, 14])
            .on_action(ButtonAction::AnalyzeCrashLog),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let content: Element<Message> = match state {
        DiagnosticsState::Idle => {
            container(text("Diagnostics run automatically when this view opens.").size(14))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .into()
        }

        DiagnosticsState::Running => container(text("Running diagnostics...").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),

        DiagnosticsState::Error(message) => container(text(message).size(14).style(danger))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),

        DiagnosticsState::Complete(report) => {
            let broken_count = report.integrity.broken_symlinks.len();
            if report.entries.is_empty() && broken_count == 0 {
                let content = column![
                    crash_controls,
                    text(format!(
                        "Profile: {} ({})",
                        report.profile_name, report.game_id
                    ))
                    .size(12),
                    text(format!("{} staged file(s) OK", report.integrity.ok_count))
                        .size(14)
                        .style(success),
                    text("No issues found!").size(14).style(success),
                ]
                .spacing(8);

                container(content)
                    .padding(20)
                    .width(Length::Fill)
                    .center_x(Length::Fill)
                    .into()
            } else {
                let rows = report
                    .entries
                    .iter()
                    .fold(column![].spacing(4), |col, entry| {
                        let (icon, icon_color): (&str, TextStyle) = match entry.severity {
                            DiagnosticSeverity::Info => ("INFO", primary),
                            DiagnosticSeverity::Warning => ("WARN", warning),
                            DiagnosticSeverity::Error => ("ERR ", danger),
                        };

                        let entry_row = row![
                            text(icon)
                                .size(12)
                                .style(icon_color)
                                .width(Length::Fixed(40.0)),
                            text(&entry.message).size(13).width(Length::Fill),
                        ]
                        .spacing(8)
                        .padding([4, 8]);

                        col.push(entry_row)
                    });

                let summary = {
                    let errors = report
                        .entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Error))
                        .count();
                    let warnings = report
                        .entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Warning))
                        .count();
                    let infos = report
                        .entries
                        .iter()
                        .filter(|e| matches!(e.severity, DiagnosticSeverity::Info))
                        .count();
                    text(format!(
                        "{} ({}) - {errors} error(s), {warnings} warning(s), {infos} info(s), {broken_count} broken symlink(s)",
                        report.profile_name, report.game_id
                    ))
                    .size(12)
                };

                let integrity = if report.integrity.broken_symlinks.is_empty() {
                    column![
                        text(format!(
                            "Integrity: {} staged file(s) OK",
                            report.integrity.ok_count
                        ))
                        .size(14)
                        .style(success)
                    ]
                } else {
                    let symlinks = report
                        .integrity
                        .broken_symlinks
                        .iter()
                        .fold(column![].spacing(2), |col, path| {
                            col.push(text(path.display().to_string()).size(12))
                        });
                    column![
                        text(format!(
                            "Integrity: {} staged file(s) OK, {} broken symlink(s)",
                            report.integrity.ok_count,
                            report.integrity.broken_symlinks.len()
                        ))
                        .size(14)
                        .style(warning),
                        container(symlinks)
                            .padding(8)
                            .width(Length::Fill)
                            .style(container::rounded_box),
                    ]
                    .spacing(6)
                };

                let crash_rows = crash_report_rows(report);

                column![
                    crash_controls,
                    summary,
                    crash_rows,
                    integrity,
                    scrollable(rows.padding(8)).height(Length::Fill),
                ]
                .spacing(8)
                .into()
            }
        }
    };

    column![title_bar, iced::widget::rule::horizontal(1), content]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn crash_report_rows(report: &DiagnosticsReport) -> Element<'_, Message> {
    let Some(crash_report) = &report.crash_report else {
        return container(text("No crash log analyzed for this diagnostics run.").size(13))
            .padding(8)
            .width(Length::Fill)
            .into();
    };
    if crash_report.suspects.is_empty() {
        return container(text("Crash log analyzed: no installed mod correlation found.").size(13))
            .padding(8)
            .width(Length::Fill)
            .into();
    }
    let rows = crash_report.suspects.iter().take(5).fold(
        column![text("Crash log correlation").size(14)].spacing(4),
        |col, suspect| {
            let name = suspect
                .display_name
                .as_deref()
                .or(suspect.mod_id.as_deref())
                .or(suspect.plugin_name.as_deref())
                .unwrap_or("unmatched crash evidence");
            let evidence = suspect
                .evidence
                .first()
                .map(|e| format!("{} in {}", e.token, e.section))
                .unwrap_or_else(|| "mentioned by crash log".to_string());
            col.push(text(format!("{}: {name} - {evidence}", suspect.confidence)).size(13))
        },
    );
    container(rows)
        .padding(8)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}
