use crate::views::selectable_text::{danger, secondary, success, text, warning};
use iced::widget::{button, column, container, mouse_area, row, scrollable};
use iced::{Alignment, Element, Length};

use modde_core::save::{SaveFingerprint, SaveSnapshot};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;

/// Render the save management view.
pub fn view<'a>(
    snapshots: &'a [SaveSnapshot],
    profile_name: Option<&'a str>,
    current_fingerprint: Option<&'a SaveFingerprint>,
    save_profiles_supported: bool,
    selected_id: Option<&'a str>,
) -> Element<'a, Message> {
    let title_bar = if save_profiles_supported {
        row![
            text("Save Management").size(20),
            iced::widget::space::horizontal(),
            button(text("Refresh").size(14))
                .style(button::secondary)
                .padding([6, 14])
                .on_action(ButtonAction::LoadSaveHistory),
        ]
        .align_y(Alignment::Center)
    } else {
        row![text("Save Management").size(20)].align_y(Alignment::Center)
    };

    let mut profile_info = column![].spacing(2);
    profile_info = profile_info.push(match profile_name {
        Some(name) => text(format!("Profile: {name}")).size(14),
        None => text("No active profile").size(14).style(warning),
    });

    if !save_profiles_supported {
        let content = container(text("Save profiles are not supported for this game.").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill);
        return column![title_bar, profile_info, content]
            .spacing(12)
            .padding(16)
            .into();
    }

    // Show current fingerprint
    if let Some(fp) = current_fingerprint
        && !fp.is_empty()
    {
        profile_info = profile_info.push(
            text(format!(
                "Save-breaking mods: {} [{}]",
                fp.mod_ids.len(),
                fp.short_hash()
            ))
            .size(12)
            .style(secondary),
        );
    }

    let header = row![
        text("Date").size(12).width(Length::Fixed(110.0)),
        text("Save").size(12).width(Length::Fill),
        text("Files").size(12).width(Length::Fixed(50.0)),
        text("Fingerprint").size(12).width(Length::Fixed(60.0)),
    ]
    .spacing(8)
    .padding([4, 0]);

    let content: Element<Message> = if snapshots.is_empty() {
        container(
            text("No save snapshots found. Saves are captured automatically when switching profiles.")
                .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let rows = snapshots.iter().fold(column![].spacing(2), |col, snap| {
            let is_selected = selected_id == Some(snap.id.as_str());

            // Date column
            let date_text = text(modde_core::save::format_timestamp_short(snap.timestamp))
                .size(12)
                .width(Length::Fixed(110.0))
                .style(secondary);

            // Save label: character + save label, or fallback to message
            let display = snap.display_title();
            let save_text = text(display).size(13).width(Length::Fill);

            let file_count = text(format!("{}", snap.file_count))
                .size(12)
                .width(Length::Fixed(50.0));

            // Fingerprint indicator
            let fp_indicator: Element<Message> = match &snap.fingerprint {
                Some(fp) if current_fingerprint.is_some() => {
                    let check = snap.check_compatibility(
                        current_fingerprint.expect("guarded by is_some() in match arm"),
                    );
                    match check {
                        modde_core::save::FingerprintCheck::Compatible => text(fp.short_hash())
                            .size(11)
                            .style(success)
                            .width(Length::Fixed(60.0))
                            .into(),
                        modde_core::save::FingerprintCheck::Mismatch {
                            ref removed,
                            ref added,
                        } => {
                            let delta = format!("-{}/+{}", removed.len(), added.len());
                            text(delta)
                                .size(11)
                                .style(danger)
                                .width(Length::Fixed(60.0))
                                .into()
                        }
                        modde_core::save::FingerprintCheck::NoFingerprint => text("—")
                            .size(11)
                            .style(secondary)
                            .width(Length::Fixed(60.0))
                            .into(),
                    }
                }
                Some(fp) => text(fp.short_hash())
                    .size(11)
                    .style(secondary)
                    .width(Length::Fixed(60.0))
                    .into(),
                None => text("—")
                    .size(11)
                    .style(secondary)
                    .width(Length::Fixed(60.0))
                    .into(),
            };

            let snapshot_row = row![date_text, save_text, file_count, fp_indicator,]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([4, 8]);

            // Wrap in a container with background highlight for selected row
            let row_container: Element<Message> = if is_selected {
                container(snapshot_row)
                    .style(container::bordered_box)
                    .width(Length::Fill)
                    .into()
            } else {
                container(snapshot_row).width(Length::Fill).into()
            };

            // Make row clickable
            let commit_id = snap.id.clone();
            let clickable =
                mouse_area(row_container).on_press(Message::SelectSaveSnapshot(commit_id));

            col.push(clickable)
        });

        scrollable(rows).height(Length::Fill).into()
    };

    let status = text(format!("{} snapshot(s)", snapshots.len())).size(12);

    column![
        title_bar,
        profile_info,
        header,
        iced::widget::rule::horizontal(1),
        content,
        status,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
