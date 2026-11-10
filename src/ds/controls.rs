//! Controls: buttons, the list filter and the overflow menu. Each returns a rune-kit builder
//! already sized for its place; the caller adds `on_press`, `help` and a key, then builds it.

use std::rc::Rc;

use rune::{El, Icon, TextEdit, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Menu, MenuItem, TextInput, glyphs};

use super::Place;

/// Three dots: rune-kit's glyph set has no "more".
static MORE: Icon = Icon {
    name: "ds-more",
    view: 16.0,
    body: r#"<circle cx="3.5" cy="8" r="1.1" fill="currentColor"/><circle cx="8" cy="8" r="1.1" fill="currentColor"/><circle cx="12.5" cy="8" r="1.1" fill="currentColor"/>"#,
};

fn with(label: impl Into<String>, place: Place, variant: ButtonVariant) -> Button {
    Button::new(label).variant(variant).control_size(place.control())
}

/// The ordinary button: outlined.
pub fn button(label: impl Into<String>, place: Place) -> Button {
    with(label, place, ButtonVariant::Outline)
}

/// The one main action of a view.
pub fn primary(label: impl Into<String>, place: Place) -> Button {
    with(label, place, ButtonVariant::Contrast)
}

/// A destructive action.
pub fn danger(label: impl Into<String>, place: Place) -> Button {
    with(label, place, ButtonVariant::Danger)
}

/// A button with no outline, for secondary actions.
pub fn quiet(label: impl Into<String>, place: Place) -> Button {
    with(label, place, ButtonVariant::Ghost)
}

/// The ordinary button with a copy icon, which becomes a check while `copied` (the app clears it
/// after `motion::COPY_CONFIRM_SECS`). While copied its accessible name is "Copied"; the visible
/// label stays.
pub fn copy_button(label: impl Into<String>, place: Place, copied: bool) -> Button {
    if !copied {
        return button(label, place).icon(&glyphs::COPY);
    }
    let label = label.into();
    button("Copied", place).icon(&glyphs::CHECK).label_with(move |font, ink| text(label).font(font).color(ink))
}

/// An icon-only button; `label` is its accessible name and tooltip.
pub fn icon_button(icon: &'static Icon, label: impl Into<String>, place: Place) -> IconButton {
    let label = label.into();
    IconButton::new(icon).variant(IconButtonVariant::GhostMuted).control_size(place.control()).label(label.clone()).help(label)
}

/// The list filter: a search field at full width with a clear button once there is text. The
/// caller adds `label`, `on_edit` and a key.
pub fn filter(edit: &TextEdit, placeholder: &str) -> TextInput {
    let mut input = TextInput::new(edit).placeholder(placeholder).leading_icon(&glyphs::SEARCH).width(None);
    if !edit.text.is_empty() {
        input = input.clear_button();
    }
    input
}

/// A toolbar title, in the kit's toolbar type, at the leading edge. It truncates before the
/// toolbar's buttons give way: the kit's centred title does not, so in the narrowest list
/// (`size::LIST_MIN`) it would sit under the buttons.
pub fn toolbar_title(k: &Kit, title: impl Into<String>) -> El {
    text(title.into()).key("title").font(k.ty.ui_medium).color(k.c.text_base).truncate()
}

/// One entry of an overflow menu.
pub struct MenuAction<S> {
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
    pub run: Box<dyn Fn(&mut S)>,
}

impl<S> MenuAction<S> {
    pub fn new(label: impl Into<String>, run: impl Fn(&mut S) + 'static) -> Self {
        MenuAction { label: label.into(), description: None, disabled: false, run: Box::new(run) }
    }

    /// A line under the label: what a button's tooltip would say.
    pub fn description(mut self, s: impl Into<String>) -> Self {
        self.description = Some(s.into());
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }
}

/// A "more" button that opens a menu of `actions`. The button is keyed `key`; the menu goes into
/// `layer`, anchors to the button, and its items are keyed `<key>-popup-item<i>` (rune-kit's
/// `Menu` fixes the item keys). Choosing an item closes the menu, then runs its action.
#[allow(clippy::too_many_arguments)] // the brief's signature plus `place`
pub fn overflow<S: 'static>(
    k: &Kit,
    key: &str,
    place: Place,
    open: bool,
    toggle: impl Fn(&mut S) + 'static,
    close: impl Fn(&mut S) + 'static,
    actions: Vec<MenuAction<S>>,
    layer: &mut Vec<El>,
) -> El {
    let trigger = icon_button(&MORE, "More actions", place).expanded(open).on_press(toggle).build(k).key(key);
    let close = Rc::new(close);
    let mut menu = Menu::new(format!("{key}-popup")).trigger(trigger).open(open).on_close({
        let close = close.clone();
        move |s: &mut S| close(s)
    });
    for a in actions {
        let close = close.clone();
        let run = a.run;
        let mut item = MenuItem::new(a.label).disabled(a.disabled);
        if let Some(d) = a.description {
            item = item.description(d);
        }
        menu = menu.item(item.on_select(move |s: &mut S| {
            close(s);
            run(s);
        }));
    }
    menu.build(k, layer)
}
