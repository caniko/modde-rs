use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::views::selectable_text::text;
use iced::widget::{button, column, container, mouse_area, opaque, row, stack, text_input};
use iced::{Element, Length, Theme};

use super::{Message, Modde, View, external_refresh_stream};

impl Modde {
    // ─── View ────────────────────────────────────────────────────

    pub(super) fn view(&self) -> Element<'_, Message> {
        // Show mod details in sidebar on all views except Saves;
        // show save details only on Saves view.
        let (mod_details_for_sidebar, save_details_for_sidebar) =
            if matches!(self.active_view, View::Saves) {
                (None, self.selected_save_details.as_ref())
            } else {
                (self.selected_mod_details.as_ref(), None)
            };
        let save_profiles_supported = self.current_game_supports_save_profiles();

        let sidebar = crate::views::sidebar::view(
            &self.active_view,
            &self.collapsed_sidebar_groups,
            &self.profiles,
            &self.active_profile,
            self.experiment_depth,
            save_profiles_supported,
            mod_details_for_sidebar,
            save_details_for_sidebar,
        );

        let mods = self
            .loaded_profile
            .as_ref()
            .map(|p| p.mods.as_slice())
            .unwrap_or(&[]);
        let settings_state = self.settings_state();

        let content: Element<Message> = match &self.active_view {
            View::Library => crate::views::library::view(&self.library),
            View::ModList => crate::views::mod_list::view_filtered(
                mods,
                &self.mod_id_filter_keys,
                &self.mod_filter,
                self.selected_mod_index,
                self.filter_mode,
                &self.filter_criteria,
                &self.collapsed_categories,
                &self.mod_categories,
                self.compact_mod_list,
                self.loaded_profile
                    .as_ref()
                    .is_some_and(|p| p.load_order_lock.is_some()),
            ),
            View::Collections => crate::views::collections::view(
                &self.collection_search,
                &self.collections,
                &self.active_downloads,
            ),
            View::BrowseNexus => {
                let domain = self.browse_game_nexus_domain();
                crate::views::browse_nexus::view(
                    &self.browse_nexus,
                    self.available_games.as_slice(),
                    domain,
                )
            }
            View::FOMODWizard(_) => crate::views::fomod_wizard::view(self),
            View::Settings => crate::views::settings::view(settings_state),
            View::WabbajackInstaller(state) => crate::views::wabbajack::view(
                state,
                &self.wabbajack_manifest,
                self.available_games.as_slice(),
                self.current_game_id(),
            ),
            View::Saves => crate::views::saves::view(
                &self.save_snapshots,
                self.loaded_profile.as_ref().map(|p| p.name.as_str()),
                self.current_fingerprint.as_ref(),
                save_profiles_supported,
                self.selected_save_details
                    .as_ref()
                    .map(|d| d.commit_id.as_str()),
            ),
            View::Downloads => {
                let tasks = self.downloads_view_tasks();
                crate::views::downloads::view(&tasks)
            }
            View::DataTab => {
                crate::views::data_tab::view(&self.data_tab_state, &self.data_tab_conflicts)
            }
            View::Diagnostics => {
                crate::views::diagnostics::view(&self.diagnostics_state, &self.crash_log_path_draft)
            }
            View::Tools => crate::views::tools::view(&self.tool_state),
            View::Executables => crate::views::executables::view(&self.tool_state),
        };

        // ── Custom title bar ──
        let game_options = crate::views::game_picker::supported_game_options_ordered(
            self.available_games.iter(),
            &self.detected_games,
        );
        let selected_game = self.selected_game.as_ref().and_then(|id| {
            game_options
                .iter()
                .find(|option| option.value == *id)
                .cloned()
        });
        let game_picker = crate::views::game_picker::game_pick_list(
            game_options,
            selected_game,
            "Select a game",
            |option| Message::SelectGame(option.value),
        );
        let add_custom_game_button = crate::semantics::test_id(
            "game_picker.add_custom_game",
            button(text("+ Add custom game").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::OpenAddCustomGame),
        );
        let manage_custom_games_button = crate::semantics::test_id(
            "game_picker.manage_custom_games",
            button(text("Manage custom games").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::OpenManageCustomGames),
        );

        let title_label = text("modde").size(14);

        let window_controls = row![
            button(text("\u{2212}").size(12))
                .style(button::secondary)
                .padding([2, 10])
                .on_action(ButtonAction::WindowMinimize),
            button(text("\u{25A1}").size(12))
                .style(button::secondary)
                .padding([2, 10])
                .on_action(ButtonAction::WindowToggleMaximize),
            button(text("\u{2715}").size(12))
                .style(button::danger)
                .padding([2, 10])
                .on_action(ButtonAction::WindowClose),
        ]
        .spacing(2);

        let title_bar_content = row![
            game_picker,
            add_custom_game_button,
            manage_custom_games_button,
            iced::widget::Space::new().width(Length::Fill),
            title_label,
            iced::widget::Space::new().width(Length::Fill),
            window_controls,
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8);

        let title_bar = mouse_area(
            container(title_bar_content)
                .padding([4, 8])
                .width(Length::Fill)
                .style(container::rounded_box),
        )
        .on_press(Message::TitleBarDrag);

        let status_bar = container(text(&self.status_message).size(12)).padding(5);

        let update_banner = self
            .update_available
            .as_ref()
            .map(crate::components::update_banner::view);

        let body: Element<Message> = if let Some(update_banner) = update_banner {
            column![
                update_banner,
                row![sidebar, content].spacing(0).height(Length::Fill)
            ]
            .spacing(0)
            .into()
        } else {
            row![sidebar, content]
                .spacing(0)
                .height(Length::Fill)
                .into()
        };

        let main_layout = column![title_bar, body, status_bar,].spacing(0);

        let mut base: Element<Message> = container(main_layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let toast_layer: Element<Message> = if let Some(toast) = self.button_hover_toast.visible {
            let toast_content: Element<Message> = container(text(toast.description).size(12))
                .padding([8, 12])
                .width(Length::Shrink)
                .style(container::rounded_box)
                .into();
            container(
                column![
                    iced::widget::Space::new().height(Length::Fill),
                    row![
                        iced::widget::Space::new().width(Length::Fill),
                        toast_content
                    ]
                ]
                .padding(16),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            container(iced::widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        base = stack([base, toast_layer]).into();

        if self.new_profile_dialog_open {
            base = stack([base, self.new_profile_dialog()]).into();
        }
        if self.game_path_dialog_open {
            base = stack([base, self.game_path_dialog()]).into();
        }
        if self.add_custom_game_dialog_open {
            base = stack([base, self.add_custom_game_modal()]).into();
        }
        if self.manage_custom_games_dialog_open {
            base = stack([base, self.manage_custom_games_modal()]).into();
        }

        crate::shortcut_layer::shortcut_layer(base).into()
    }

    fn new_profile_dialog(&self) -> Element<'_, Message> {
        let trimmed_name = self.new_profile_name.trim();
        let can_create = !trimmed_name.is_empty() && self.selected_game.is_some();
        let submit = can_create.then_some(Message::SubmitNewProfileDialog);
        let submit_action = can_create.then_some(ButtonAction::SubmitNewProfileDialog);

        let dialog = container(
            column![
                text("New Profile").size(18),
                text_input("Profile name...", &self.new_profile_name)
                    .on_input(Message::NewProfileNameChanged)
                    .on_submit_maybe(submit.clone())
                    .padding(8)
                    .width(Length::Fill),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .style(button::secondary)
                        .padding([6, 14])
                        .on_action(ButtonAction::CancelNewProfileDialog),
                    button(text("Create").size(13))
                        .style(button::success)
                        .padding([6, 14])
                        .on_action_maybe(
                            submit_action,
                            "Enter a profile name and select a game before creating the profile.",
                        ),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(12),
        )
        .width(Length::Fixed(360.0))
        .padding(16)
        .style(container::rounded_box);

        opaque(
            container(dialog)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
    }

    fn game_path_dialog(&self) -> Element<'_, Message> {
        let game_id = self
            .pending_game_path_game_id
            .as_deref()
            .unwrap_or("the selected game");
        let game_label = modde_games::resolve_game_plugin(game_id)
            .map(modde_games::GamePlugin::display_name)
            .unwrap_or(game_id);

        let mut body = column![
            text("Game Path Required").size(18),
            text(format!(
                "modde could not detect {game_label}. Select the game installation directory to continue."
            ))
            .size(13),
        ]
        .spacing(12);

        if let Some(error) = &self.game_path_dialog_error {
            body = body.push(text(error).size(12).color(iced::color!(0xFF6666)));
        }

        let dialog = container(
            body.push(
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .style(button::secondary)
                        .padding([6, 14])
                        .on_action(ButtonAction::CancelGamePathDialog),
                    button(text("Browse").size(13))
                        .style(button::primary)
                        .padding([6, 14])
                        .on_action(ButtonAction::GamePathDialogBrowse),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
        )
        .width(Length::Fixed(420.0))
        .padding(16)
        .style(container::rounded_box);

        opaque(
            container(dialog)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
    }

    pub(super) fn theme(&self) -> Theme {
        match self.theme_name.as_str() {
            "Light" => Theme::Light,
            "Dracula" => Theme::Dracula,
            "Nord" => Theme::Nord,
            "Gruvbox Dark" => Theme::GruvboxDark,
            "Catppuccin Mocha" => Theme::CatppuccinMocha,
            _ => Theme::Dark,
        }
    }

    /// Crate-internal accessor for the headless screenshot backend, which
    /// lives in the sibling `crate::screenshot` module and so cannot reach
    /// `view()`/`theme()` directly. Only compiled with the `screenshot`
    /// feature so the default build is unaffected.
    #[cfg(feature = "screenshot")]
    pub(crate) fn render_root(&self) -> Element<'_, Message> {
        self.view()
    }

    /// Crate-internal accessor for the active `iced::Theme`. See
    /// [`Self::render_root`].
    #[cfg(feature = "screenshot")]
    pub(crate) fn active_theme(&self) -> Theme {
        self.theme()
    }

    /// Subscribe to external refresh signals from the CLI.
    ///
    /// We bind a Unix domain socket at [`modde_core::ipc::socket_path`]
    /// and emit a [`Message::ExternalRefresh`] for every connection
    /// received. The socket is cleaned up on startup (in case a
    /// previous GUI crashed without unlinking) and on each new bind.
    /// While idle, the listener costs nothing — `accept()` just blocks
    /// in the kernel.
    pub(super) fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            iced::Subscription::run(external_refresh_stream),
            iced::time::every(std::time::Duration::from_secs(2))
                .map(|_| Message::LibrarySessionTick),
        ])
    }
}
