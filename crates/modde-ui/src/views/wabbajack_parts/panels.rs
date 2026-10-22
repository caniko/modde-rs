#![allow(clippy::wildcard_imports)]
use super::*;
use iced::widget::column;

pub(super) fn action_controls(state: &WabbajackInstallerState) -> Element<'_, Message> {
    let recheck_action = state
        .file_path
        .as_ref()
        .map(|_| ButtonAction::WabbajackCheckReadiness);
    let import_action = state
        .file_path
        .as_ref()
        .map(|_| ButtonAction::WabbajackImportArchives);
    let install_action = state
        .can_install()
        .then_some(ButtonAction::WabbajackStartInstall);

    column![
        row![
            semantics::test_id(
                "wabbajack.download",
                button(text("Download").size(12))
                    .style(button::primary)
                    .padding([4, 10])
                    .on_action(ButtonAction::WabbajackDownloadSelected),
            ),
            semantics::test_id(
                "wabbajack.recheck",
                button(text("Recheck").size(12))
                    .style(button::secondary)
                    .padding([4, 10])
                    .on_action_maybe(
                        recheck_action,
                        "Select or download a local .wabbajack file before checking readiness.",
                    ),
            ),
            semantics::test_id(
                "wabbajack.import_archives",
                button(text("Import archives").size(12))
                    .style(button::secondary)
                    .padding([4, 10])
                    .on_action_maybe(
                        import_action,
                        "Select a .wabbajack file before importing manual archives.",
                    ),
            ),
            semantics::test_id(
                "wabbajack.install",
                button(text("Install").size(12))
                    .style(button::success)
                    .padding([4, 10])
                    .on_action_maybe(
                        install_action,
                        "Resolve readiness blockers before installing."
                    ),
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        state.install_blocker().map_or_else(
            || text("Ready to install.").size(11).style(success),
            |blocker| text(blocker).size(11).style(warning),
        ),
    ]
    .spacing(4)
    .into()
}

pub(super) fn target_controls(state: &WabbajackInstallerState) -> Element<'_, Message> {
    column![
        text("Install Target").size(13),
        text_input("profile", &state.hm_profile)
            .id(semantics::widget_id("wabbajack.input.hm_profile"))
            .on_input(Message::WabbajackHmProfileChanged)
            .padding(5),
        text_input("game", &state.hm_game)
            .id(semantics::widget_id("wabbajack.input.hm_game"))
            .on_input(Message::WabbajackHmGameChanged)
            .padding(5),
        text_input("gameDir (optional)", &state.hm_game_dir)
            .id(semantics::widget_id("wabbajack.input.hm_game_dir"))
            .on_input(Message::WabbajackHmGameDirChanged)
            .padding(5),
    ]
    .spacing(5)
    .into()
}

pub(super) fn readiness_panel(state: &WabbajackInstallerState) -> Element<'_, Message> {
    let mut panel = column![text("Readiness").size(13)].spacing(4);
    if state.readiness_loading {
        panel = panel.push(text("Checking selected Wabbajack file...").size(11));
    } else if let Some(error) = &state.readiness_error {
        panel = panel.push(
            text(format!("Readiness check failed: {error}"))
                .size(11)
                .style(danger),
        );
    } else if let Some(report) = &state.readiness {
        let ready_text = if report.install_ready {
            "Ready for validated staging/deploy"
        } else {
            "Not ready to install"
        };
        let ready_color: TextStyle = if report.install_ready {
            success
        } else {
            warning
        };
        panel = panel
            .push(text(ready_text).size(12).style(ready_color))
            .push(
                text(format!(
                    "{} archive(s), {} directive(s), game {}",
                    report.archives, report.directives, report.normalized_game
                ))
                .size(11),
            )
            .push(
                text(format!(
                    "Staging: {} ({})",
                    report.staging.layout_action, report.staging.path
                ))
                .size(11),
            )
            .push(
                text(format!(
                    "Game-file sources: {}/{} present",
                    report.game_file_sources.present, report.game_file_sources.total
                ))
                .size(11),
            )
            .push(
                text(format!(
                    "Nexus: {}",
                    if !report.nexus_required {
                        "not required"
                    } else if report.nexus_available {
                        "configured"
                    } else {
                        "missing API key"
                    }
                ))
                .size(11),
            );

        for blocker in report.hard_blockers.iter().take(5) {
            panel = panel.push(text(format!("Blocker: {blocker}")).size(11).style(danger));
        }
        for message in report.warnings.iter().take(5) {
            panel = panel.push(text(format!("Warning: {message}")).size(11).style(warning));
        }
    } else {
        panel = panel
            .push(text("Select or download a .wabbajack file to run readiness checks.").size(11));
    }
    container(semantics::test_id("wabbajack.panel.readiness", panel))
        .padding(8)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

pub(super) fn missing_archives_panel(state: &WabbajackInstallerState) -> Element<'_, Message> {
    let mut panel = column![text("Missing Manual Archives").size(13)].spacing(4);
    if let Some(status) = &state.archive_import_status {
        panel = panel.push(text(status).size(11).style(primary));
    }
    if !state.archive_import_results.is_empty() {
        for result in state.archive_import_results.iter().rev().take(5) {
            panel = panel.push(
                text(format!(
                    "{}: {} ({:016x})",
                    result.source_path.display(),
                    result.status,
                    result.computed_xxh64
                ))
                .size(10),
            );
        }
    }
    if let Some(report) = &state.readiness {
        if report.manual_downloads.is_empty() {
            panel = panel.push(
                text("No unresolved manual archives.")
                    .size(11)
                    .style(success),
            );
        } else {
            for (index, archive) in report.manual_downloads.iter().take(8).enumerate() {
                panel = panel.push(
                    container(
                        column![
                            text(&archive.name).size(11),
                            text(format!("hash {}", archive.hash))
                                .size(10)
                                .style(secondary),
                            row![
                                semantics::test_id(
                                    format!("wabbajack.manual_archive.{index}.open_url"),
                                    button(text("Open URL").size(11))
                                        .style(button::secondary)
                                        .padding([3, 8])
                                        .on_action(ButtonAction::WabbajackOpenUrl(
                                            archive.url.clone()
                                        )),
                                ),
                                text("Download it, then use Import archives.").size(10),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(2),
                    )
                    .padding(6)
                    .width(Length::Fill)
                    .style(container::rounded_box),
                );
            }
            if report.manual_downloads.len() > 8 {
                panel = panel.push(
                    text(format!(
                        "... and {} more manual archive(s)",
                        report.manual_downloads.len() - 8
                    ))
                    .size(10),
                );
            }
        }
    } else {
        panel = panel.push(text("Run readiness to list manual archives.").size(11));
    }
    container(semantics::test_id("wabbajack.panel.manual_archives", panel))
        .padding(8)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

pub(super) fn progress_panel(state: &WabbajackInstallerState) -> Element<'_, Message> {
    let pct = state.progress * 100.0;
    let log_content = if state.log_lines.is_empty() {
        column![text("Waiting for activity...").size(11)]
    } else {
        state
            .log_lines
            .iter()
            .rev()
            .take(80)
            .fold(column![].spacing(2), |col, line| {
                col.push(text(line).size(11))
            })
    };
    container(semantics::test_id(
        "wabbajack.panel.progress",
        column![
            text("Install Progress").size(13),
            text(if state.install_phase.is_empty() {
                "Idle".to_string()
            } else if state.install_current_item.is_empty() {
                state.install_phase.clone()
            } else {
                format!("{}: {}", state.install_phase, state.install_current_item)
            })
            .size(11),
            progress_bar(0.0..=100.0, pct).girth(10),
            text("Log").size(12),
            container(scrollable(log_content).height(Length::Fixed(140.0))).padding(6),
        ]
        .spacing(4),
    ))
    .padding(8)
    .width(Length::Fill)
    .style(container::rounded_box)
    .into()
}

pub(super) fn hm_controls<'a>(state: &'a WabbajackInstallerState) -> Element<'a, Message> {
    let snippet_preview: Element<'a, Message> = if state.hm_snippet.is_empty() {
        text("Generate a snippet to preview it here.")
            .size(11)
            .style(secondary)
            .into()
    } else {
        container(scrollable(text(&state.hm_snippet).size(10)).height(Length::Fixed(140.0)))
            .padding(8)
            .width(Length::Fill)
            .style(container::rounded_box)
            .into()
    };

    column![
        text("Home Manager").size(13),
        text("Uses the install target fields above.")
            .size(11)
            .style(secondary),
        row![
            button(text("Generate").size(12))
                .padding([4, 8])
                .on_action(ButtonAction::WabbajackGenerateHmSnippet),
            button(text("Copy").size(12))
                .padding([4, 8])
                .on_action_maybe(
                    (!state.hm_snippet.is_empty()).then_some(ButtonAction::WabbajackCopyHmSnippet),
                    "Generate a Home Manager snippet before copying it.",
                ),
            button(text("Save").size(12))
                .padding([4, 8])
                .on_action_maybe(
                    (!state.hm_snippet.is_empty()).then_some(ButtonAction::WabbajackSaveHmSnippet),
                    "Generate a Home Manager snippet before saving it.",
                ),
        ]
        .spacing(6),
        snippet_preview,
    ]
    .spacing(5)
    .into()
}

pub(super) fn selected_entry(state: &WabbajackInstallerState) -> Option<&WabbajackCatalogEntry> {
    state.selected_index.and_then(|idx| state.entries.get(idx))
}

pub(super) fn format_size_summary(entry: &WabbajackCatalogEntry) -> String {
    let mut parts = Vec::new();
    if let Some(count) = entry.size.archive_count {
        parts.push(format!("{count} archive(s)"));
    }
    if let Some(size) = entry.size.total_size.or(entry.size.archive_size) {
        parts.push(format_bytes(size));
    }
    if parts.is_empty() {
        "Size: unknown".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
