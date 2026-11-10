//! Controls: buttons, the list filter and the overflow menu. Each returns a rune-kit builder
//! already sized for its place; the caller adds `on_press`, `help` and a key, then builds it.

use std::rc::Rc;

use rune::{El, Icon, TextEdit, Tx, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Menu, MenuItem, SegmentedControl, Switch, TextInput, glyphs};

use super::{Place, space};

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

/// A `quiet` button with an eye that shows or hides a card's masked values: "Show values" while
/// they are hidden, "Hide values" while `shown`. The caller adds `help`, `on_press` and a key.
pub fn reveal(shown: bool, place: Place) -> Button {
    let (label, icon) = if shown { ("Hide values", &glyphs::EYE_OFF) } else { ("Show values", &glyphs::EYE) };
    quiet(label, place).icon(icon)
}

/// A switch at today's size: switches are not buttons, so `Place` does not apply. Dark gets the
/// kit's compact switch at its default (Mini, 36 × 16); light gets the standard switch, because
/// off, the compact one disappears on a light pane (and the standard one on a dark pane). The
/// caller adds `label`, `hide_label`, `on_toggle`, then builds and keys it.
pub fn switch(k: &Kit, on: bool) -> Switch {
    let s = Switch::new(on);
    if k.dark { s } else { s.standard() }
}

/// A text field whose text is masked until its eye is pressed, the eye after the field
/// ("Show value" / "Hide value"). Build it with `secret_input`; `key` keys the field, `eye_key`
/// the eye.
pub struct SecretInput {
    input: TextInput,
    eye: IconButton,
    key: Option<String>,
    eye_key: Option<String>,
}

/// A `SecretInput` over `edit`, shown when `shown`; `on_toggle` flips `shown`. The field takes
/// the width left beside the eye.
pub fn secret_input<S: 'static>(edit: &TextEdit, on_edit: impl Fn(&mut S) -> &mut TextEdit + 'static, shown: bool, on_toggle: impl Fn(&mut S) + 'static) -> SecretInput {
    let mut s = SecretInput::unwired(edit, shown, on_toggle);
    s.input = s.input.on_edit(on_edit);
    s
}

impl SecretInput {
    /// The field with no edit handler, so typing changes nothing: the gallery's, which has no
    /// state to edit.
    pub(super) fn unwired<S: 'static>(edit: &TextEdit, shown: bool, on_toggle: impl Fn(&mut S) + 'static) -> SecretInput {
        let input = TextInput::new(edit).width(None).secure().revealed(shown);
        let (label, icon) = if shown { ("Hide value", &glyphs::EYE_OFF) } else { ("Show value", &glyphs::EYE) };
        SecretInput { input, eye: icon_button(icon, label, Place::Inspector).on_press(on_toggle), key: None, eye_key: None }
    }

    /// The field's accessible name.
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.input = self.input.label(l);
        self
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.input = self.input.placeholder(p);
        self
    }

    pub fn invalid(mut self, on: bool) -> Self {
        self.input = self.input.invalid(on);
        self
    }

    pub fn on_submit<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.input = self.input.on_submit(f);
        self
    }

    /// Keys the field.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Keys the eye.
    pub fn eye_key(mut self, key: impl Into<String>) -> Self {
        self.eye_key = Some(key.into());
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let mut input = self.input.build(k);
        if let Some(key) = self.key {
            input = input.key(key);
        }
        let mut eye = self.eye.build(k);
        if let Some(key) = self.eye_key {
            eye = eye.key(key);
        }
        rune::row().w_full().gap(space::S1).center_y().child(input.grow(1.0).min_w(0.0)).child(eye)
    }
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

/// Segments that narrow a list ("All 12", "Broken 1"), for `Page::segments`: as wide as the
/// labels need, at the list's control height (the kit's segments are 28, `Place::List`). `label`
/// is the accessible name of the choice ("Show"). The caller adds a key, then builds it.
pub fn segments<S: 'static>(labels: &[String], current: Option<usize>, label: &str, on_pick: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static) -> SegmentedControl {
    SegmentedControl::new(labels, current).label(label).fit().on_pick(on_pick)
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
