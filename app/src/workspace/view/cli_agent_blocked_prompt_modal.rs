use warpui::elements::{
    Border, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Flex,
    MainAxisSize, ParentElement, Radius, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::keymap::FixedBinding;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
use crate::terminal::{CLIAgent, TerminalView};
use crate::ui_components::blended_colors;
use crate::ui_components::dialog::{dialog_styles, Dialog};
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{ActionButton, PrimaryTheme, SecondaryTheme};

const DIALOG_WIDTH: f32 = 600.;
const DIALOG_PADDING: f32 = 24.;
const FOOTER_BUTTON_GAP: f32 = 8.;
const FOOTER_BUTTON_WIDTH: f32 = (DIALOG_WIDTH - DIALOG_PADDING * 2. - FOOTER_BUTTON_GAP) / 2.;
const MAX_PREVIEW_CHARS: usize = 900;
const PREVIEW_MAX_HEIGHT: f32 = 220.;

pub(crate) fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;

    app.register_fixed_bindings([
        FixedBinding::new(
            "escape",
            CLIAgentBlockedPromptModalAction::Cancel,
            id!(CLIAgentBlockedPromptModal::ui_name()),
        ),
        FixedBinding::new(
            "a",
            CLIAgentBlockedPromptModalAction::Proceed,
            id!(CLIAgentBlockedPromptModal::ui_name()),
        ),
        FixedBinding::new(
            "b",
            CLIAgentBlockedPromptModalAction::Cancel,
            id!(CLIAgentBlockedPromptModal::ui_name()),
        ),
        FixedBinding::new(
            "enter",
            CLIAgentBlockedPromptModalAction::Proceed,
            id!(CLIAgentBlockedPromptModal::ui_name()),
        ),
    ]);
}

#[derive(Clone)]
pub(crate) struct CLIAgentBlockedPromptModalSource {
    pub terminal_view: ViewHandle<TerminalView>,
    pub agent: CLIAgent,
    pub message: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input_preview: Option<String>,
}

pub(crate) enum CLIAgentBlockedPromptModalEvent {
    Proceed {
        terminal_view: ViewHandle<TerminalView>,
    },
    Cancel {
        terminal_view: ViewHandle<TerminalView>,
    },
}

#[derive(Debug)]
pub(crate) enum CLIAgentBlockedPromptModalAction {
    Proceed,
    Cancel,
}

pub(crate) struct CLIAgentBlockedPromptModal {
    proceed_button: ViewHandle<ActionButton>,
    cancel_button: ViewHandle<ActionButton>,
    source: Option<CLIAgentBlockedPromptModalSource>,
}

impl CLIAgentBlockedPromptModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let proceed_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Allow and continue", PrimaryTheme)
                .with_icon(Icon::Check)
                .with_full_width(true)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentBlockedPromptModalAction::Proceed);
                })
        });

        let cancel_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Tell Codex no", SecondaryTheme)
                .with_icon(Icon::X)
                .with_full_width(true)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentBlockedPromptModalAction::Cancel);
                })
        });

        Self {
            proceed_button,
            cancel_button,
            source: None,
        }
    }

    pub fn set_source(
        &mut self,
        source: CLIAgentBlockedPromptModalSource,
        ctx: &mut ViewContext<Self>,
    ) {
        self.source = Some(source);
        ctx.notify();
    }

    pub fn clear(&mut self, ctx: &mut ViewContext<Self>) {
        if self.source.take().is_some() {
            ctx.notify();
        }
    }

    pub fn is_open(&self) -> bool {
        self.source.is_some()
    }

    pub fn source_terminal_view_id(&self) -> Option<warpui::EntityId> {
        self.source.as_ref().map(|source| source.terminal_view.id())
    }

    fn title(&self) -> String {
        let Some(source) = self.source.as_ref() else {
            return "CLI agent is waiting".to_owned();
        };

        let agent_name = source.agent.display_name();
        let Some(tool_name) = self.tool_name() else {
            return format!("{agent_name} needs confirmation");
        };

        if is_command_tool_name(tool_name) {
            format!("Allow {agent_name} to run {tool_name}?")
        } else {
            format!("Allow {agent_name} to use {tool_name}?")
        }
    }

    fn message(&self) -> String {
        let Some(source) = self.source.as_ref() else {
            return "The CLI agent is waiting for your response.".to_owned();
        };

        source.message.clone().unwrap_or_else(|| {
            format!(
                "{} is waiting for your response.",
                source.agent.display_name()
            )
        })
    }

    fn tool_name(&self) -> Option<&str> {
        self.source
            .as_ref()
            .and_then(|source| source.tool_name.as_deref())
            .map(str::trim)
            .filter(|tool_name| !tool_name.is_empty())
    }

    fn tool_input_preview(&self) -> Option<String> {
        self.source
            .as_ref()
            .and_then(|source| source.tool_input_preview.as_deref())
            .map(str::trim)
            .filter(|preview| !preview.is_empty())
            .map(truncate_preview)
    }

    fn render_content(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let background = theme.surface_1();
        let text_color = blended_colors::text_sub(theme, background);
        let preview = self.tool_input_preview();
        let message = self.message();
        let should_show_message =
            !message_is_redundant(&message, preview.as_deref(), self.tool_name());

        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.);

        if should_show_message {
            content.add_child(
                Text::new(message, appearance.ui_font_family(), 14.)
                    .with_color(text_color)
                    .finish(),
            );
        }

        if let Some(preview) = preview {
            content.add_child(self.render_preview(preview, appearance));
        }

        content.finish()
    }

    fn render_preview(&self, preview: String, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let panel_background = blended_colors::neutral_1(theme);
        let border_color = blended_colors::neutral_4(theme);
        let label_color = blended_colors::text_sub(theme, panel_background);
        let code_color = blended_colors::text_main(theme, panel_background);
        let label = if self.tool_name().is_some_and(is_command_tool_name) {
            "Command"
        } else {
            "Input"
        };

        let header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_child(
                ConstrainedBox::new(Icon::Terminal.to_warpui_icon(label_color.into()).finish())
                    .with_width(13.)
                    .with_height(13.)
                    .finish(),
            )
            .with_child(
                Text::new_inline(label.to_owned(), appearance.ui_font_family(), 12.)
                    .with_style(Properties::default().weight(Weight::Medium))
                    .with_color(label_color)
                    .finish(),
            )
            .finish();

        let preview_text = ConstrainedBox::new(
            Text::new(
                preview,
                appearance.monospace_font_family(),
                appearance.monospace_font_size() - 1.,
            )
            .with_color(code_color)
            .finish(),
        )
        .with_max_height(PREVIEW_MAX_HEIGHT)
        .finish();

        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(10.)
            .with_child(header)
            .with_child(preview_text)
            .finish();

        Container::new(content)
            .with_padding_left(12.)
            .with_padding_top(12.)
            .with_padding_right(12.)
            .with_padding_bottom(12.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .with_background_color(panel_background)
            .with_border(Border::all(1.).with_border_fill(border_color))
            .finish()
    }
}

impl Entity for CLIAgentBlockedPromptModal {
    type Event = CLIAgentBlockedPromptModalEvent;
}

impl View for CLIAgentBlockedPromptModal {
    fn ui_name() -> &'static str {
        "CLIAgentBlockedPromptModal"
    }

    fn on_focus(&mut self, _focus_ctx: &warpui::FocusContext, ctx: &mut ViewContext<Self>) {
        ctx.focus_self();
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);

        let proceed_button =
            evenly_sized_footer_button(ChildView::new(&self.proceed_button).finish(), true);
        let cancel_button =
            evenly_sized_footer_button(ChildView::new(&self.cancel_button).finish(), false);

        Dialog::new(
            self.title(),
            None,
            UiComponentStyles {
                width: Some(DIALOG_WIDTH),
                padding: Some(Coords::uniform(DIALOG_PADDING)),
                ..dialog_styles(appearance)
            },
        )
        .with_child(self.render_content(appearance))
        .with_separator()
        .with_bottom_row_child(proceed_button)
        .with_bottom_row_child(cancel_button)
        .build()
        .finish()
    }
}

impl TypedActionView for CLIAgentBlockedPromptModal {
    type Action = CLIAgentBlockedPromptModalAction;

    fn handle_action(
        &mut self,
        action: &CLIAgentBlockedPromptModalAction,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(source) = self.source.clone() else {
            log::error!("CLI agent blocked prompt action received with no source");
            return;
        };

        match action {
            CLIAgentBlockedPromptModalAction::Proceed => {
                ctx.emit(CLIAgentBlockedPromptModalEvent::Proceed {
                    terminal_view: source.terminal_view,
                });
            }
            CLIAgentBlockedPromptModalAction::Cancel => {
                ctx.emit(CLIAgentBlockedPromptModalEvent::Cancel {
                    terminal_view: source.terminal_view,
                });
            }
        }
    }
}

fn truncate_preview(preview: &str) -> String {
    let mut result = String::new();
    for (index, ch) in preview.chars().enumerate() {
        if index == MAX_PREVIEW_CHARS {
            result.push_str("...");
            return result;
        }
        result.push(ch);
    }
    result
}

fn evenly_sized_footer_button(button: Box<dyn Element>, add_right_gap: bool) -> Box<dyn Element> {
    let button = ConstrainedBox::new(button)
        .with_width(FOOTER_BUTTON_WIDTH)
        .finish();
    let mut container = Container::new(button);
    if add_right_gap {
        container = container.with_margin_right(FOOTER_BUTTON_GAP);
    }
    container.finish()
}

fn is_command_tool_name(tool_name: &str) -> bool {
    let tool_name = tool_name.to_ascii_lowercase();
    matches!(
        tool_name.as_str(),
        "bash" | "sh" | "zsh" | "fish" | "powershell" | "pwsh" | "cmd"
    ) || tool_name.contains("command")
        || tool_name.contains("shell")
        || tool_name.contains("exec")
        || tool_name.contains("terminal")
}

fn message_is_redundant(message: &str, preview: Option<&str>, tool_name: Option<&str>) -> bool {
    let message = message.trim();
    let has_preview = preview.is_some();

    if let Some(preview) = preview.map(str::trim) {
        if preview.len() > 12 && message.contains(preview) {
            return true;
        }
    }

    if has_preview && message.ends_with("is waiting for your response.") {
        return true;
    }

    if let (true, Some(tool_name)) = (has_preview, tool_name) {
        let expected_prefix = format!("wants to run {}", tool_name.to_ascii_lowercase());
        if message.to_ascii_lowercase().starts_with(&expected_prefix) {
            return true;
        }
    }

    false
}
