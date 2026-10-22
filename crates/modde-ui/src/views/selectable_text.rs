use iced::advanced::clipboard::{self, Clipboard};
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::text::paragraph;
use iced::advanced::widget::text as text_widget;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Renderer as _, text::Paragraph as _};
use iced::advanced::{Shell, Widget, text as advanced_text};
use iced::event::Event;
use iced::keyboard::{self, Key};
use iced::{
    Background, Border, Color, Element, Font, Length, Pixels, Point, Rectangle, Shadow, Size, Theme,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::Message;

type Renderer = iced::Renderer;

pub use text_widget::base;

/// A theme style chosen at runtime, e.g. `let s: TextStyle = if ok { success } else { danger };`.
pub type TextStyle = fn(&Theme) -> text_widget::Style;

/// WCAG AA contrast for body text. iced's own `is_readable_on` wants 6:1, which
/// saturated accent hues cannot reach on dark backgrounds without going grey.
pub(crate) const MIN_TEXT_CONTRAST: f32 = 4.5;

/// Nudge a palette colour towards readability on the theme background while
/// keeping its hue. iced's Dark `success` (`#12664f`) is a fill colour, not a
/// text colour, so used raw it vanishes on dark cards.
fn on_background(theme: &Theme, color: Color) -> Color {
    use iced::theme::palette::{darken, is_dark, lighten, mix};
    let background = theme.palette().background;
    let dark = is_dark(background);
    let steps = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
    // `lighten`/`darken` keep saturation but bump chroma and can leave gamut
    // for strong accents; `mix` towards the pole always converges, at the
    // cost of hue. Try the saturated route first.
    let improved = steps.map(|amount| {
        if dark {
            lighten(color, amount)
        } else {
            darken(color, amount)
        }
    });
    let mixed = steps.map(|amount| {
        mix(
            color,
            if dark { Color::WHITE } else { Color::BLACK },
            amount,
        )
    });
    std::iter::once(color)
        .chain(improved)
        .chain(mixed)
        .find(|candidate| background.relative_contrast(*candidate) >= MIN_TEXT_CONTRAST)
        .unwrap_or(color)
}

macro_rules! palette_style {
    ($(#[$doc:meta])* $name:ident => $field:ident) => {
        $(#[$doc])*
        pub fn $name(theme: &Theme) -> text_widget::Style {
            text_widget::Style {
                color: Some(on_background(theme, theme.palette().$field)),
            }
        }
    };
}

// Theme-aware text styles used as `text("…").style(danger)`. They follow the
// active theme, unlike a hardcoded `.color(..)`.
palette_style!(/// Accent / link colour.
    primary => primary);
palette_style!(/// Positive state.
    success => success);
palette_style!(/// Needs attention but not failed.
    warning => warning);
palette_style!(/// Errors and destructive actions.
    danger => danger);

/// Muted captions and metadata.
pub fn secondary(theme: &Theme) -> text_widget::Style {
    text_widget::Style {
        color: Some(on_background(
            theme,
            theme.extended_palette().secondary.base.color,
        )),
    }
}

pub struct SelectableText {
    content: String,
    format: text_widget::Format<Font>,
    style: Option<text_widget::StyleFn<'static, Theme>>,
}

#[derive(Debug, Default)]
struct State {
    paragraph: paragraph::Plain<<Renderer as advanced_text::Renderer>::Paragraph>,
    focus: bool,
    drag_anchor: Option<usize>,
    selection: Option<(usize, usize)>,
}

pub fn text(content: impl ToString) -> SelectableText {
    SelectableText {
        content: content.to_string(),
        format: text_widget::Format::default(),
        style: None,
    }
}

impl SelectableText {
    #[must_use]
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.format.size = Some(size.into());
        self
    }

    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.format.width = width.into();
        self
    }

    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.format.height = height.into();
        self
    }

    /// Colour the text from the active theme, e.g. `.style(danger)`.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> text_widget::Style + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }

    /// Fixed colour that ignores the theme. Prefer [`Self::style`].
    #[must_use]
    pub fn color(self, color: impl Into<Color>) -> Self {
        let color = color.into();
        self.style(move |_| text_widget::Style { color: Some(color) })
    }
}

impl Widget<Message, Theme, Renderer> for SelectableText {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.format.width,
            height: self.format.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        text_widget::layout(
            &mut state.paragraph,
            renderer,
            limits,
            &self.content,
            self.format,
        )
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_over(bounds) {
                    state.focus = true;
                    let index = hit_index(&state.paragraph, bounds, position).unwrap_or(0);
                    state.drag_anchor = Some(index);
                    state.selection = None;
                } else {
                    state.focus = false;
                    state.drag_anchor = None;
                    state.selection = None;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(anchor), Some(position)) =
                    (state.drag_anchor, cursor.position_over(bounds))
                    && let Some(index) = hit_index(&state.paragraph, bounds, position)
                    && anchor != index
                {
                    state.selection = Some((anchor, index));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let had_selection = state.selection.is_some();
                state.drag_anchor = None;

                if had_selection {
                    shell.capture_event();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if state.focus
                    && modifiers.command()
                    && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("c"))
                    && let Some((start, end)) = normalized_selection(state.selection)
                {
                    clipboard.write(
                        clipboard::Kind::Standard,
                        selected_text(&self.content, start, end),
                    );
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        if let Some((start, end)) = normalized_selection(state.selection) {
            draw_selection(renderer, theme, bounds, state, &self.content, start, end);
        }

        text_widget::draw(
            renderer,
            defaults,
            bounds,
            state.paragraph.raw(),
            self.style
                .as_ref()
                .map_or_else(text_widget::Style::default, |style| style(theme)),
            viewport,
        );
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.text(None, layout.bounds(), &self.content);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl From<SelectableText> for Element<'_, Message> {
    fn from(text: SelectableText) -> Self {
        Element::new(text)
    }
}

fn hit_index(
    paragraph: &paragraph::Plain<<Renderer as advanced_text::Renderer>::Paragraph>,
    bounds: Rectangle,
    position: Point,
) -> Option<usize> {
    let anchor = bounds.anchor(
        paragraph.min_bounds(),
        paragraph.align_x(),
        paragraph.align_y(),
    );
    paragraph
        .raw()
        .hit_test(Point::new(position.x - anchor.x, position.y - anchor.y))
        .map(advanced_text::Hit::cursor)
}

fn normalized_selection(selection: Option<(usize, usize)>) -> Option<(usize, usize)> {
    selection
        .map(|(start, end)| (start.min(end), start.max(end)))
        .filter(|(start, end)| start != end)
}

fn selected_text(content: &str, start: usize, end: usize) -> String {
    UnicodeSegmentation::graphemes(content, true)
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn draw_selection(
    renderer: &mut Renderer,
    theme: &Theme,
    bounds: Rectangle,
    state: &State,
    content: &str,
    start: usize,
    end: usize,
) {
    let paragraph = state.paragraph.raw();
    let anchor = bounds.anchor(
        state.paragraph.min_bounds(),
        state.paragraph.align_x(),
        state.paragraph.align_y(),
    );
    let line_height = paragraph.line_height().to_absolute(paragraph.size()).0;
    let palette = theme.extended_palette();

    let mut line_start = 0;
    for (line_index, line) in content.split_inclusive('\n').enumerate() {
        let has_newline = line.ends_with('\n');
        let line_content = line.trim_end_matches('\n');
        let line_len = UnicodeSegmentation::graphemes(line_content, true).count();
        let line_end = line_start + line_len;

        let selection_start = start.max(line_start);
        let selection_end = end.min(line_end);

        if selection_start < selection_end {
            let local_start = selection_start - line_start;
            let local_end = selection_end - line_start;

            if let (Some(start_pos), Some(end_pos)) = (
                paragraph.grapheme_position(line_index, local_start),
                paragraph.grapheme_position(line_index, local_end),
            ) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: anchor.x + start_pos.x,
                            y: anchor.y + start_pos.y,
                            width: (end_pos.x - start_pos.x).max(1.0),
                            height: line_height,
                        },
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: true,
                    },
                    Background::Color(palette.primary.weak.color),
                );
            }
        }

        line_start = line_end + usize::from(has_newline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_styles_are_readable_on_every_theme() {
        for (name, theme) in &crate::app::THEMES {
            let background = theme.palette().background;
            for (style_name, style) in [
                ("primary", primary as TextStyle),
                ("success", success),
                ("warning", warning),
                ("danger", danger),
                ("secondary", secondary),
            ] {
                let color = style(theme)
                    .color
                    .expect("palette styles always set a colour");
                let contrast = background.relative_contrast(color);
                assert!(
                    contrast >= MIN_TEXT_CONTRAST,
                    "{style_name} has {contrast:.2}:1 on the {name} theme"
                );
            }
        }
    }
}
