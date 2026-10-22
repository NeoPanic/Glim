use super::Workspace;
use crate::app::actions::{CheckForUpdates, DefaultApplications, KeyboardShortcuts};
use glim_services::preferences::Appearance;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, Sizable,
        button::Button,
        h_flex,
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
        v_flex,
    },
    *,
};

#[derive(Clone, Copy)]
enum PreferenceChoice {
    Theme(Appearance),
    Language(&'static str),
}

fn preference_picker(
    id: &'static str,
    label: &'static str,
    value: &'static str,
    options: Vec<(PreferenceChoice, &'static str)>,
    cx: &App,
) -> AnyElement {
    Button::new(id)
        .small()
        .min_w(px(110.))
        .h(px(30.))
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(8.))
        .disabled(crate::app::updates::busy(cx))
        .accessibility_label(format!(
            "{}: {}",
            glim_services::locale::tr(label),
            glim_services::locale::tr(value)
        ))
        .child(
            h_flex()
                .gap_2()
                .child(glim_services::locale::tr(value))
                .child(Icon::new(gpui_kit::assets::IconName::ChevronDown).size(px(14.))),
        )
        .dropdown_menu(move |mut menu, _, _| {
            for (choice, label) in &options {
                let choice = *choice;
                menu = menu.item(
                    PopupMenuItem::new(glim_services::locale::tr(label)).on_click(
                        move |_, _, cx| match choice {
                            PreferenceChoice::Theme(mode) => crate::app::set_appearance(mode, cx),
                            PreferenceChoice::Language(language) => {
                                crate::app::change_language(language, cx)
                            }
                        },
                    ),
                );
            }
            menu
        })
        .into_any_element()
}

impl Workspace {
    pub(super) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let appearance = cx.global::<crate::app::AppearancePreference>().0;
        let theme = preference_picker(
            "settings-theme",
            "Theme",
            match appearance {
                Appearance::System => "Follow System",
                Appearance::Light => "Light",
                Appearance::Dark => "Dark",
            },
            vec![
                (PreferenceChoice::Theme(Appearance::System), "Follow System"),
                (PreferenceChoice::Theme(Appearance::Light), "Light"),
                (PreferenceChoice::Theme(Appearance::Dark), "Dark"),
            ],
            cx,
        );
        let languages = glim_services::locale::available();
        let current = cx.global::<crate::app::LanguagePreference>().0.as_str();
        let label = languages
            .iter()
            .find(|(code, _)| *code == current)
            .map_or("Follow System", |(_, name)| *name);
        let mut options = vec![(PreferenceChoice::Language("system"), "Follow System")];
        options.extend(
            languages
                .into_iter()
                .map(|(code, name)| (PreferenceChoice::Language(code), name)),
        );
        let language = preference_picker("settings-language", "Language", label, options, cx);
        let action_button = |id: &'static str, label: &'static str| {
            Button::new(id)
                .small()
                .min_w(px(110.))
                .h(px(30.))
                .border_1()
                .border_color(cx.theme().border)
                .rounded(px(8.))
                .disabled(crate::app::updates::busy(cx))
                .label(glim_services::locale::tr(label))
        };
        let associations = action_button("settings-file-types", "Manage…")
            .accessibility_label(glim_services::locale::tr("Default File Types…"))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(DefaultApplications), cx))
            .into_any_element();
        let updates = action_button("settings-updates", crate::app::updates::check_label(cx))
            .disabled(crate::app::updates::busy(cx) || crate::app::updates::checking(cx))
            .accessibility_label(glim_services::locale::tr("Check for Updates…"))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(CheckForUpdates), cx))
            .into_any_element();
        let shortcuts = action_button("settings-shortcuts", "View…")
            .accessibility_label(glim_services::locale::tr("Keyboard Shortcuts…"))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(KeyboardShortcuts), cx))
            .into_any_element();
        let telemetry = Switch::new("settings-telemetry")
            .small()
            .checked(cx.global::<crate::app::telemetry::Telemetry>().enabled)
            .accessibility_label(glim_services::locale::tr("Anonymous usage telemetry"))
            .on_click(|enabled, _, cx| crate::app::telemetry::set_enabled(*enabled, cx))
            .into_any_element();
        let row = |label: &'static str, detail: &'static str, control: AnyElement| {
            let mut text = v_flex()
                .flex_1()
                .gap_1()
                .child(div().text_sm().child(glim_services::locale::tr(label)));
            if !detail.is_empty() {
                text = text.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(glim_services::locale::tr(detail)),
                );
            }
            h_flex()
                .w_full()
                .min_h(px(58.))
                .py_3()
                .gap_4()
                .child(text)
                .child(control)
        };
        let group = || {
            v_flex().w_full().px_4().rounded(px(12.)).bg(cx
                .theme()
                .background
                .blend(cx.theme().foreground.opacity(0.035)))
        };
        let divider = || div().w_full().h(px(1.)).bg(cx.theme().border);
        let heading = |label: &'static str| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(glim_services::locale::tr(label))
        };
        div()
            .id("workspace-settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex().w_full().items_center().px_6().py_6().child(
                    v_flex()
                        .w_full()
                        .max_w(px(560.))
                        .gap_3()
                        .child(heading("General"))
                        .child(
                            group()
                                .child(row("Theme", "", theme))
                                .child(divider())
                                .child(row("Language", "", language)),
                        )
                        .child(heading("Files").mt_3())
                        .child(group().child(row(
                            "Default File Types…",
                            "Choose which file types open in Glim.",
                            associations,
                        )))
                        .child(heading("Application").mt_3())
                        .child(
                            group()
                                .child(row("Check for Updates…", "", updates))
                                .child(divider())
                                .child(row("Keyboard Shortcuts…", "", shortcuts))
                                .child(divider())
                                .child(row("Anonymous usage telemetry", "", telemetry)),
                        ),
                ),
            )
            .into_any_element()
    }
}
