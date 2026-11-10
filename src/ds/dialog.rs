//! The dialog frame: the one block built on raw Rune instead of wrapping rune-kit's `Dialog`,
//! which is centred, has radius 6, a smaller title and no footer. The modal wiring (focus trap,
//! Escape and outside press, focus return, the dialog role) is the kit's, copied.

use rune::{El, ScrollSpec, SpringPair, Transition, col, el, icon, row};
use rune_kit::{Kit, glyphs};

use super::{Place, color, icon_button, motion, radius, size, space, text};

/// Which of the two widths a dialog has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// A question before an action: 440.
    Confirm,
    /// A form: 560.
    Form,
}

impl DialogKind {
    fn width(self) -> f32 {
        match self {
            DialogKind::Confirm => size::DIALOG_CONFIRM,
            DialogKind::Form => size::DIALOG_FORM,
        }
    }
}

/// A modal dialog: title and close button, a body that scrolls when the window is too short,
/// an optional error line and a footer of buttons. Its top sits at `size::DIALOG_TOP` whatever
/// its height, so dialogs open in the same place.
pub struct DialogFrame {
    key: String,
    title: String,
    kind: DialogKind,
    body: Vec<El>,
    error: Option<String>,
    footer: Option<(El, El)>,
    menus: Vec<El>,
}

impl DialogFrame {
    pub fn new(key: impl Into<String>, title: impl Into<String>, kind: DialogKind) -> Self {
        DialogFrame { key: key.into(), title: title.into(), kind, body: Vec::new(), error: None, footer: None, menus: Vec::new() }
    }

    /// A block of the body; blocks stack with a gap of `S4`.
    pub fn body(mut self, e: El) -> Self {
        self.body.push(e);
        self
    }

    /// A message shown above the footer with a danger icon.
    pub fn error(mut self, e: Option<String>) -> Self {
        self.error = e;
        self
    }

    /// Cancel and the primary action, right-aligned. Build them with `Place::Bar`.
    pub fn footer(mut self, cancel: El, primary: El) -> Self {
        self.footer = Some((cancel, primary));
        self
    }

    /// Dropdown menus of fields in the body. They go into the layer after the dialog, because
    /// a menu anchors to its field, which has to be laid out first.
    pub fn menus(mut self, mut m: Vec<El>) -> Self {
        self.menus.append(&mut m);
        self
    }

    /// Pushes the scrim, the dialog and then its menus into `layer`. `on_close` runs on Escape,
    /// a press outside the dialog and the close button.
    pub fn build<S: 'static>(self, k: &Kit, on_close: impl Fn(&mut S) + Clone + 'static, layer: &mut Vec<El>) {
        let springs = SpringPair::new(motion::enter(), motion::exit());
        let key = self.key;

        let close = icon_button(&glyphs::CLOSE, "Close", Place::Inspector).on_press(on_close.clone()).build(k).key("close");
        // The button is taller than the title's line; the negative margins keep the header's
        // height the title's.
        let header = row()
            .w_full()
            .padding(space::S4)
            .gap(space::S2)
            .items_start()
            .child(text::title(k, self.title.clone()).grow(1.0).min_w(0.0))
            .child(close.my(-2.0));

        let content = col().w_full().px(space::S4).gap(space::S4).children(self.body);
        let body = col().key(format!("{key}-body")).w_full().shrink().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(content);

        let mut panel = col()
            .key(format!("{key}-dialog"))
            .role("dialog")
            .label(self.title)
            .w_full()
            .max_w(self.kind.width())
            .shrink()
            .min_h(0.0)
            .radius(radius::DIALOG)
            .fill(color::card(k))
            .child(header)
            .child(body);
        if let Some(e) = self.error {
            panel = panel.child(
                row()
                    .key(format!("{key}-error"))
                    .w_full()
                    .px(space::S4)
                    .pt(space::S4)
                    .gap(space::S2)
                    .items_start()
                    .child(icon(&glyphs::CIRCLE_X, size::ICON).color(color::bad(k)).mt(2.0))
                    .child(text::body(k, e).grow(1.0).min_w(0.0)),
            );
        }
        match self.footer {
            Some((cancel, primary)) => {
                panel = panel.child(row().w_full().padding(space::S4).gap(space::S2).justify_end().child(cancel).child(primary));
            }
            None => panel = panel.child(el().h(space::S4)),
        }
        let panel = k.elevate(panel, &k.elevation.overlay).transition(Transition::scale(0.97).springs(springs)).modal(true, on_close);

        layer.push(
            el().key(format!("{key}-scrim")).absolute(0.0, 0.0).w_full().h_full().fill(k.c.scrim).transition(Transition::opacity().springs(springs)).a11y_hidden(),
        );
        // Top at DIALOG_TOP, 16 from the other edges; the panel shrinks to what is left and
        // its body scrolls.
        layer.push(
            col()
                .key(format!("{key}-layer"))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .pt(size::DIALOG_TOP)
                .px(space::S4)
                .pb(space::S4)
                .items_center()
                .transition(Transition::identity().springs(springs))
                .child(panel),
        );
        layer.extend(self.menus);
    }
}
