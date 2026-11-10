//! Design tokens: spacing, radii, sizes, type roles, colour roles and motion. Components in
//! `ds` take every number and colour from here, so a change lands in one place.

use rune::{Color, Spring, TextStyle};
use rune_kit::shell::PaneMotion;
use rune_kit::{ControlSize, Kit};

/// Spacing scale in points.
pub mod space {
    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
    pub const S6: f32 = 24.0;
    /// Gap between list rows.
    pub const ROW_GAP: f32 = 2.0;
}

/// Corner radii in points.
pub mod radius {
    pub const TAG: f32 = 4.0;
    pub const CONTROL: f32 = 6.0;
    pub const CARD: f32 = 8.0;
    pub const DIALOG: f32 = 12.0;
}

/// Fixed measurements in points.
pub mod size {
    pub const ROW_1: f32 = 32.0;
    pub const ROW_2: f32 = 44.0;
    pub const DOT: f32 = 7.0;
    pub const ICON: f32 = 16.0;
    pub const DIALOG_CONFIRM: f32 = 440.0;
    pub const DIALOG_FORM: f32 = 560.0;
    pub const DIALOG_TOP: f32 = 96.0;
    /// The inspector's narrowest width: the Keys action row ("Remove from agent", "…", "Move to
    /// Trash…") fits at 352, not at the 320 first planned.
    pub const INSPECTOR_MIN: f32 = 352.0;
    pub const INSPECTOR_MAX: f32 = 400.0;
    /// The list's room in the inspector rule: below sidebar + this + `INSPECTOR_MIN` the
    /// inspector hides. 276 = the 860 minimum window − the 232 default sidebar − `INSPECTOR_MIN`,
    /// so the minimum window shows all three panes. The inspector shrinks (to `INSPECTOR_MIN` at
    /// the least) to leave the list this much, so a stored width up to `INSPECTOR_MAX` never
    /// cramps it.
    pub const LIST_MIN: f32 = 276.0;
    pub const PANE_EDGE: f32 = 16.0;
    pub const PAIR_LABEL: f32 = 120.0;
    /// A list row's name column never gets narrower than this; its tags give way first.
    pub const ROW_NAME_MIN: f32 = 120.0;
    /// A short field beside a wide one: a port.
    pub const FIELD_SHORT: f32 = 96.0;
    /// How far a focused field's ring reaches outside the field.
    pub const FOCUS_RING: f32 = 3.0;
}

/// Where a control sits, which fixes its height: 24 in the inspector, 28 in lists, filters,
/// toolbars and dialog footers (rune-kit has no 32-point control).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Inspector,
    List,
    Bar,
}

impl Place {
    /// The kit's size for a control here: `Regular` is 24 points tall, `Large` 28.
    pub fn control(self) -> ControlSize {
        match self {
            Place::Inspector => ControlSize::Regular,
            Place::List | Place::Bar => ControlSize::Large,
        }
    }
}

/// Type roles. Families come from the kit, so the app's fonts carry over; only size, weight and
/// line height are set here.
pub mod ty {
    use super::*;

    fn with(base: TextStyle, size: f32, weight: f32, line_height: f32) -> TextStyle {
        TextStyle { size, weight, line_height, ..base }
    }

    pub fn title(k: &Kit) -> TextStyle {
        with(k.ty.ui, 15.0, 600.0, 20.0)
    }
    pub fn value(k: &Kit) -> TextStyle {
        with(k.ty.ui, 13.0, 400.0, 20.0)
    }
    /// A value in medium weight: list names.
    pub fn name(k: &Kit) -> TextStyle {
        TextStyle { weight: 500.0, ..value(k) }
    }
    pub fn code(k: &Kit) -> TextStyle {
        with(k.ty.mono, 12.0, 400.0, 18.0)
    }
    pub fn label(k: &Kit) -> TextStyle {
        with(k.ty.small, 11.0, 400.0, 16.0)
    }
    /// Same style as `label`; it differs only in colour (`color::text_faint`).
    pub fn hint(k: &Kit) -> TextStyle {
        label(k)
    }
}

/// Colour roles. Text is base, muted or faint; status colours are for dots and icons.
pub mod color {
    use super::*;

    pub fn text(k: &Kit) -> Color {
        k.c.text_base
    }
    pub fn text_muted(k: &Kit) -> Color {
        k.c.text_muted
    }
    pub fn text_faint(k: &Kit) -> Color {
        k.c.text_faint
    }
    /// The pane a page or inspector sits on.
    pub fn pane(k: &Kit) -> Color {
        k.c.bg_base
    }
    pub fn card(k: &Kit) -> Color {
        k.c.layer_01
    }
    pub fn hairline(k: &Kit) -> Color {
        k.c.border_muted
    }
    pub fn selected(k: &Kit) -> Color {
        k.c.layer_02
    }
    pub fn hover(k: &Kit) -> Color {
        k.c.hover
    }
    pub fn ok(k: &Kit) -> Color {
        k.c.success.icon
    }
    pub fn warn(k: &Kit) -> Color {
        k.c.warning.icon
    }
    pub fn bad(k: &Kit) -> Color {
        k.c.danger.icon
    }
    pub fn info(k: &Kit) -> Color {
        k.c.info.icon
    }
}

/// Motion: short, no bounce, and only opacity when the user asks to reduce motion.
pub mod motion {
    use super::*;

    /// Opacity of a de-emphasised row (a pattern, an unused entry).
    pub const DIM: f32 = 0.75;
    pub const COPY_CONFIRM_SECS: f64 = 1.2;

    pub fn press_scale(reduced: bool) -> f32 {
        if reduced { 1.0 } else { 0.97 }
    }
    /// Things appearing: ~180 ms.
    pub fn enter() -> Spring {
        Spring::new(0.18, 0.0)
    }
    /// Things leaving: ~120 ms.
    pub fn exit() -> Spring {
        Spring::new(0.12, 0.0)
    }
    /// Inspector content changing: ~150 ms.
    pub fn swap() -> Spring {
        Spring::new(0.15, 0.0)
    }
    /// Side panes opening and closing: ~300 ms for a 480-point pane, 150 to 300 ms by width;
    /// contents fade only near closed. Under reduced motion the panes snap.
    pub fn panes() -> PaneMotion {
        PaneMotion::new().spring(Spring::new(0.3, 0.0)).scale_with_width(480.0, 0.15, 0.3).fade_tail(0.15)
    }
}
