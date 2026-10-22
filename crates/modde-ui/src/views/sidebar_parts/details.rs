#![allow(clippy::wildcard_imports)]
use super::*;
use iced::widget::column;

pub(super) fn render_mod_details(state: &ModDetailsState) -> Element<'_, Message> {
    // Loading state — show only a minimal placeholder, no partial data.
    if state.loading {
        return column![
            text(&state.name).size(13),
            text("Loading…").size(11).style(secondary),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into();
    }

    // Error state — show the error instead of the metadata block.
    if let Some(ref err) = state.error {
        return column![
            text(&state.name).size(13),
            text(err.as_str()).size(11).style(danger),
            button(text("Open in Nexus").size(11))
                .style(button::text)
                .padding([2, 4])
                .on_action(ButtonAction::OpenModPage),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into();
    }

    // ── Thumbnail ──
    let thumb_slot: Element<Message> = match &state.thumbnail {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(96.0))
            .content_fit(iced::ContentFit::Contain)
            .into(),
        None => container(text("…").size(14).style(secondary))
            .width(Length::Fill)
            .height(Length::Fixed(96.0))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(96.0))
            .style(container::bordered_box)
            .into(),
    };

    // Wrap the thumbnail in a mouse_area so clicking cycles the gallery.
    // Only attach the on_press handler when there's actually more than one
    // image to cycle through.
    let thumb_area: Element<Message> = if state.gallery.len() > 1 {
        mouse_area(thumb_slot)
            .on_press(Message::ModGalleryNext)
            .into()
    } else {
        thumb_slot
    };

    // Gallery position indicator ("2 / 5") — only shown when multiple images.
    let gallery_indicator: Element<Message> = if state.gallery.len() > 1 {
        text(format!(
            "{} / {}",
            state.gallery_index + 1,
            state.gallery.len()
        ))
        .size(10)
        .style(secondary)
        .into()
    } else {
        iced::widget::Space::new().into()
    };

    // ── Metadata text ──
    let author_version: Element<Message> = if state.author.is_empty() {
        text(&state.version).size(11).style(secondary).into()
    } else {
        text(format!("by {} · v{}", state.author, state.version))
            .size(11)
            .style(secondary)
            .into()
    };

    // Summary — clamp length so it doesn't blow out the sidebar.
    let summary_text: Element<Message> = match state.summary.as_deref() {
        Some(s) if !s.is_empty() => {
            let truncated = if s.chars().count() > SUMMARY_MAX {
                let mut t: String = s.chars().take(SUMMARY_MAX).collect();
                t.push('…');
                t
            } else {
                s.to_string()
            };
            text(truncated).size(11).into()
        }
        _ => iced::widget::Space::new().into(),
    };

    // ── Endorse / Track action buttons ──
    // Both buttons reflect the user's current relationship with the mod on
    // Nexus. While any action is in flight (`action_pending`), both buttons
    // lose their `on_press` to block double-submits. Before the initial
    // fetch resolves (`endorse_status`/`is_tracked` are None), the buttons
    // render but are disabled.
    let disabled = state.action_pending;

    let endorsed = state.endorse_status.as_deref() == Some("Endorsed");
    let endorse_label = if endorsed { "✓ Endorsed" } else { "Endorse" };
    let endorse_style = if endorsed {
        button::success
    } else if state.endorse_status.is_some() {
        button::primary
    } else {
        button::secondary
    };
    let endorse_btn = button(text(endorse_label).size(11))
        .style(endorse_style)
        .padding([3, 8])
        .width(Length::Fill)
        .on_action_maybe(
            (!disabled && state.endorse_status.is_some()).then_some(ButtonAction::ModEndorseToggle),
            "Nexus endorsement status is still loading or an action is already in progress.",
        );

    let tracked = state.is_tracked == Some(true);
    let track_label = if tracked { "Tracked" } else { "Track" };
    let track_style = if tracked {
        button::success
    } else if state.is_tracked.is_some() {
        button::primary
    } else {
        button::secondary
    };
    let track_btn = button(text(track_label).size(11))
        .style(track_style)
        .padding([3, 8])
        .width(Length::Fill)
        .on_action_maybe(
            (!disabled && state.is_tracked.is_some()).then_some(ButtonAction::ModTrackToggle),
            "Nexus tracking status is still loading or an action is already in progress.",
        );

    let action_row = row![endorse_btn, track_btn].spacing(4);

    // Endorsement count line — only rendered when > 0 to keep the panel
    // uncluttered for mods that have just been published.
    let count_line: Element<Message> = if state.endorsement_count > 0 {
        text(format!("{} endorsements", state.endorsement_count))
            .size(10)
            .style(secondary)
            .into()
    } else {
        iced::widget::Space::new().into()
    };

    let link_button = button(text("Open in Nexus").size(11))
        .style(button::text)
        .padding([2, 4])
        .on_action(ButtonAction::OpenModPage);

    column![
        thumb_area,
        gallery_indicator,
        text(&state.name).size(13),
        author_version,
        summary_text,
        action_row,
        count_line,
        link_button,
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

/// Render the save detail panel appended to the bottom of the left sidebar.
pub(super) fn render_save_details(state: &SaveDetailsState) -> Element<'_, Message> {
    use iced::widget::scrollable;
    use modde_core::save::FingerprintCheck;

    let mut col = column![].spacing(4).width(Length::Fill);

    // Date
    col = col.push(text(state.formatted_date()).size(12).style(secondary));

    // Title: character + save label
    col = col.push(text(state.display_title()).size(13));

    // Category badge
    if let Some(ref cat) = state.category {
        col = col.push(text(format!("[{cat}]")).size(11).style(secondary));
    }

    // Profile name
    if let Some(ref name) = state.profile_name {
        col = col.push(text(format!("Profile: {name}")).size(11).style(secondary));
    }

    // File count
    col = col.push(text(format!("{} file(s)", state.file_count)).size(11));

    // File list
    match &state.file_paths {
        Some(paths) if !paths.is_empty() => {
            let file_list = paths.iter().fold(column![].spacing(1), |col, path| {
                // Show only the filename for brevity in the narrow sidebar
                let display = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                col.push(text(display).size(10).style(secondary))
            });
            col = col.push(scrollable(file_list).height(Length::Fixed(80.0)));
        }
        Some(_) => {} // empty list, skip
        None => {
            col = col.push(text("Loading files...").size(10).style(secondary));
        }
    }

    // Fingerprint + compatibility
    if let Some(ref fp) = state.fingerprint {
        let fp_element: Element<Message> = match &state.compatibility {
            Some(FingerprintCheck::Compatible) => {
                text(format!("Mods: {} [compatible]", fp.short_hash()))
                    .size(11)
                    .style(success)
                    .into()
            }
            Some(FingerprintCheck::Mismatch { removed, added }) => column![
                text(format!("Mods: {} [mismatch]", fp.short_hash()))
                    .size(11)
                    .style(danger),
                text(format!(
                    "-{} removed, +{} added",
                    removed.len(),
                    added.len()
                ))
                .size(10)
                .style(danger),
            ]
            .spacing(1)
            .into(),
            Some(FingerprintCheck::NoFingerprint) | None => {
                text(format!("Mods: {}", fp.short_hash()))
                    .size(11)
                    .style(secondary)
                    .into()
            }
        };
        col = col.push(fp_element);
    }

    // Restore button
    col = col.push(
        button(text("Restore").size(12))
            .style(button::secondary)
            .padding([4, 8])
            .width(Length::Fill)
            .on_action(ButtonAction::RestoreSaveSnapshot(state.commit_id.clone())),
    );

    // Commit ID (subtle)
    col = col.push(text(&state.short_id).size(10).style(secondary));

    col.into()
}
