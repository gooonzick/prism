//! The list page: filter, status line, a scrolling list of rows (or an empty state), all on one
//! content edge. Plus the row that goes in the list.

use rune::{Cursor, El, Icon, ScrollSpec, Tx, col, el, icon, row};
use rune_kit::{Checkbox, Kit, Spinner, glyphs};

use super::{color, motion, radius, size, space, text, ty};

/// A list page. The filter, the segments, the count (or selection line), the notices and the
/// footer stay put; the rows scroll and take the remaining height.
pub struct Page {
    key: String,
    filter: Option<El>,
    segments: Option<El>,
    count: Option<String>,
    selection: Option<El>,
    notices: Vec<El>,
    rows: Vec<El>,
    empty: Option<El>,
    footer: Option<El>,
}

impl Page {
    pub fn new(key: impl Into<String>) -> Self {
        Page {
            key: key.into(),
            filter: None,
            segments: None,
            count: None,
            selection: None,
            notices: Vec::new(),
            rows: Vec::new(),
            empty: None,
            footer: None,
        }
    }

    pub fn filter(mut self, e: El) -> Self {
        self.filter = Some(e);
        self
    }

    /// Segments that narrow the list (`ds::segments`), between the filter and the status line,
    /// at their own width.
    pub fn segments(mut self, e: El) -> Self {
        self.segments = Some(e);
        self
    }

    /// The first line of the status line, in place of the count: a `Selection` line.
    pub fn selection(mut self, e: El) -> Self {
        self.selection = Some(e);
        self
    }

    /// A block under the list that does not scroll: a label and a row of controls.
    pub fn footer(mut self, e: El) -> Self {
        self.footer = Some(e);
        self
    }

    /// The count line ("12 hosts"), keyed `count`.
    pub fn count(mut self, s: impl Into<String>) -> Self {
        self.count = Some(s.into());
        self
    }

    /// A notice under the count; several stack.
    pub fn notice(mut self, n: El) -> Self {
        self.notices.push(n);
        self
    }

    pub fn rows(mut self, it: impl IntoIterator<Item = El>) -> Self {
        self.rows.extend(it);
        self
    }

    /// Shown in place of the list when there are no rows.
    pub fn empty(mut self, e: El) -> Self {
        self.empty = Some(e);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let mut page = col().key(self.key).w_full().h_full().min_h(0.0).padding(size::PANE_EDGE).gap(space::S3);
        if let Some(f) = self.filter {
            page = page.child(f);
        }
        if let Some(s) = self.segments {
            // In a row, so the segments keep the width their labels need; narrower than that, they
            // shrink to the content edge, share it equally and truncate their labels.
            page = page.child(row().w_full().child(s.shrink()));
        }
        let first = self.selection.or_else(|| self.count.map(|c| text::label(k, c).key("count")));
        if first.is_some() || !self.notices.is_empty() {
            let mut status = col().w_full().gap(space::S2);
            if let Some(f) = first {
                status = status.child(f);
            }
            for n in self.notices {
                status = status.child(n);
            }
            page = page.child(status);
        }
        if self.rows.is_empty() {
            return match (self.empty, self.footer) {
                (Some(e), None) => page.child(e),
                (None, None) => page,
                // The empty state takes the list's room, so the footer stays at the bottom.
                (e, Some(f)) => page.child(col().w_full().grow(1.0).min_h(0.0).children(e)).child(f),
            };
        }
        // Rows paint a highlight `S2` past the content edge. The scroll area clips, so it is
        // `S2` wider than the content on each side and pads that back; the rows own their edge.
        let list = col()
            .key("list")
            .self_stretch()
            .min_h(0.0)
            .grow(1.0)
            .mx(-space::S2)
            .px(space::S2)
            .items_stretch()
            .gap(space::ROW_GAP)
            .scroll(ScrollSpec { stick_bottom: false })
            .children(self.rows);
        let page = page.child(list);
        match self.footer {
            Some(f) => page.child(f),
            None => page,
        }
    }
}

/// A handler that takes only the state.
type Handler<S> = Box<dyn Fn(&mut S)>;

/// The first line of a page's status line when its rows can be picked (`Page::selection`): a
/// select-all checkbox on the same edge as the rows' checkboxes, then a summary, or with rows
/// picked "N selected" and a clear button (the bulk bar); then actions, and a `danger` pinned
/// right. The checkbox is keyed `check-all`, the bar `bulk-bar`, the clear button `bulk-clear`.
pub struct Selection<S> {
    all: bool,
    partial: bool,
    enabled: bool,
    on_toggle_all: Handler<S>,
    label: String,
    summary: Option<String>,
    picked: Option<(usize, Handler<S>)>,
    clear_help: String,
    actions: Vec<El>,
    danger: Option<El>,
}

impl<S: 'static> Selection<S> {
    /// `all`: every row is picked; `partial`: some are (the checkbox shows a dash); `enabled`:
    /// there are rows to pick.
    pub fn new(all: bool, partial: bool, enabled: bool, on_toggle_all: impl Fn(&mut S) + 'static) -> Self {
        Selection {
            all,
            partial,
            enabled,
            on_toggle_all: Box::new(on_toggle_all),
            label: "Select all".into(),
            summary: None,
            picked: None,
            clear_help: "Clear selection".into(),
            actions: Vec::new(),
            danger: None,
        }
    }

    /// The select-all checkbox's accessible name (default "Select all").
    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into();
        self
    }

    /// What the list holds, as a Label that truncates; shown while nothing is picked.
    pub fn summary(mut self, s: impl Into<String>) -> Self {
        self.summary = Some(s.into());
        self
    }

    /// `n` rows are picked: the line becomes the bulk bar, "N selected" and a clear button.
    pub fn picked(mut self, n: usize, on_clear: impl Fn(&mut S) + 'static) -> Self {
        self.picked = Some((n, Box::new(on_clear)));
        self
    }

    /// The clear button's tooltip (default "Clear selection"), for a shortcut that also clears.
    pub fn clear_help(mut self, s: impl Into<String>) -> Self {
        self.clear_help = s.into();
        self
    }

    /// A button after the summary or the "N selected" line; several follow in order.
    pub fn action(mut self, b: El) -> Self {
        self.actions.push(b);
        self
    }

    /// A destructive button, pinned to the right end of the line.
    pub fn danger(mut self, b: El) -> Self {
        self.danger = Some(b);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let mut all = Checkbox::new(self.all).indeterminate(self.partial);
        if self.partial {
            // The kit draws its check for a mixed state unless given another mark.
            all = all.icon(&glyphs::DASH);
        }
        let all = all.label(self.label).hide_label().disabled(!self.enabled).on_toggle(self.on_toggle_all).build(k).key("check-all");
        let danger = self.danger.map(|d| row().grow(1.0).justify_end().center_y().child(d));
        let rest = match self.picked {
            Some((n, on_clear)) => {
                // "N selected ×" is one target: pressing the words clears too.
                let clear = row()
                    .key("bulk-clear")
                    .role("button")
                    .label("Clear selection")
                    .help(self.clear_help)
                    .focusable(true)
                    .cursor(Cursor::Pointer)
                    .h(size::ROW_1)
                    .gap(space::S1)
                    .center_y()
                    .on_press(on_clear)
                    .child(text::name(k, format!("{n} selected")))
                    .child(icon(&glyphs::CLOSE, size::ICON).color(color::text_muted(k)));
                // Wraps when the buttons don't fit; the danger button keeps to the right.
                row()
                    .key("bulk-bar")
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(space::S2)
                    .center_y()
                    .flex_wrap()
                    .child(clear)
                    .children(self.actions)
                    .children(danger)
            }
            None => {
                let summary = match self.summary {
                    Some(s) => text::label(k, s).grow(1.0).min_w(0.0),
                    None => row().grow(1.0),
                };
                row().grow(1.0).min_w(0.0).h(size::ROW_1).gap(space::S2).center_y().child(summary).children(self.actions).children(danger)
            }
        };
        // `ROW_1` high in both states, so the list does not move when the first row is picked. When
        // the bar wraps, the checkbox stays on its first line.
        let all = row().h(size::ROW_1).center_y().child(all);
        row().key("selection").w_full().gap(space::S3).items_start().child(all).child(rest)
    }
}

/// One row of a list: an optional icon, a name, an optional meta line and tags.
pub struct ListRow {
    key: String,
    name: String,
    icon: Option<&'static Icon>,
    meta: Option<String>,
    tags: Vec<El>,
    selected: bool,
    dim: bool,
    reduced: bool,
    pick: Option<(bool, Checkbox)>,
    pick_key: Option<String>,
    trailing: Option<El>,
}

impl ListRow {
    pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
        ListRow {
            key: key.into(),
            name: name.into(),
            icon: None,
            meta: None,
            tags: Vec::new(),
            selected: false,
            dim: false,
            reduced: false,
            pick: None,
            pick_key: None,
            trailing: None,
        }
    }

    /// A leading checkbox for picking several rows; `label` is its accessible name ("Select
    /// work"), and `on_toggle` gets the `Tx` for the modifiers held. A picked row that is not
    /// selected fills `color::checked`; the selected fill wins. The checkbox is keyed
    /// `<key>-check` unless `pick_key` names it.
    pub fn pick<S: 'static>(mut self, on: bool, label: impl Into<String>, on_toggle: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.pick = Some((on, Checkbox::new(on).label(label).hide_label().on_toggle_tx(on_toggle)));
        self
    }

    /// The key of the `pick` checkbox.
    pub fn pick_key(mut self, key: impl Into<String>) -> Self {
        self.pick_key = Some(key.into());
        self
    }

    /// A column at the row's end, after the tags, `size::ROW_TRAILING` wide with its content at
    /// the end, so statuses line up from row to row. Keyed `<key>-trailing`.
    pub fn trailing(mut self, e: El) -> Self {
        self.trailing = Some(e);
        self
    }

    pub fn icon(mut self, i: &'static Icon) -> Self {
        self.icon = Some(i);
        self
    }

    /// A second line; with it the row is `ROW_2` high, without it `ROW_1`.
    pub fn meta(mut self, s: impl Into<String>) -> Self {
        self.meta = Some(s.into());
        self
    }

    pub fn tags(mut self, it: impl IntoIterator<Item = El>) -> Self {
        self.tags.extend(it);
        self
    }

    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }

    pub fn dim(mut self, on: bool) -> Self {
        self.dim = on;
        self
    }

    /// Reduced motion: the press keeps its size and only changes opacity.
    pub fn reduced(mut self, on: bool) -> Self {
        self.reduced = on;
        self
    }

    /// The name is keyed `<key>-name`. The row has `S2` of negative margin and `S2` of padding:
    /// its icon and name sit on the parent's content edge and its highlight reaches `S2` past it.
    /// The parent must stretch its children (a `Page` list does).
    pub fn build<S: 'static>(self, k: &Kit, on_press: impl Fn(&mut S) + 'static) -> El {
        self.frame(k).on_press(on_press)
    }

    /// `build`, with the press's `Tx`: the modifiers held (⌘- and ⇧-click to pick).
    pub fn build_tx<S: 'static>(self, k: &Kit, on_press: impl Fn(&mut S, &mut Tx) + 'static) -> El {
        self.frame(k).on_press_tx(on_press)
    }

    /// The row without its press handler.
    fn frame(self, k: &Kit) -> El {
        let check_key = self.pick_key.unwrap_or_else(|| format!("{}-check", self.key));
        let trailing_key = format!("{}-trailing", self.key);
        let height = if self.meta.is_some() { size::ROW_2 } else { size::ROW_1 };
        // The name column's floor is `ROW_NAME_MIN`, or all the room the name has when the row
        // is narrower than its checkbox, icon, `ROW_NAME_MIN` and trailing column. Its only
        // in-flow child is an invisible `ROW_NAME_MIN` strut, so that is its min-content (the
        // name and meta lines are pinned over it and add none, however long), and its full width
        // in the middle caps that floor, as CSS's automatic minimum size does (`min(content,
        // specified width)`). So the tags give way first, then the name truncates; the trailing
        // column never does.
        let mut lines = col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).justify_center().child(text::name(k, self.name.clone()).key(format!("{}-name", self.key)));
        if let Some(m) = self.meta {
            lines = lines.child(text::label(k, m));
        }
        let body = col().grow(1.0).w_frac(1.0).h_full().child(el().w(size::ROW_NAME_MIN).h(0.0).a11y_hidden()).child(lines);
        let mut r = row()
            .key(self.key)
            .role("button")
            .label(self.name)
            .focusable(true)
            .h(height)
            .mx(-space::S2)
            .px(space::S2)
            .gap(space::S3)
            .center_y()
            .radius(radius::CONTROL)
            .press_scale(motion::press_scale(self.reduced));
        let picked = self.pick.as_ref().is_some_and(|p| p.0);
        if let Some((_, check)) = self.pick {
            r = r.child(check.build(k).key(check_key));
        }
        if let Some(i) = self.icon {
            r = r.child(icon(i, size::ICON).color(color::text_muted(k)));
        }
        let mut middle = row().grow(1.0).min_w(0.0).h_full().gap(space::S3).center_y().child(body);
        if !self.tags.is_empty() {
            // Tags give way to the name: past `ROW_NAME_MIN` they narrow and are cut at the end.
            middle = middle.child(row().shrink().gap(space::S1).center_y().clip_rounded(radius::TAG).children(self.tags));
        }
        r = r.child(middle);
        if let Some(t) = self.trailing {
            r = r.child(row().key(trailing_key).w(size::ROW_TRAILING).center_y().justify_end().child(t));
        }
        r = match (self.selected, picked) {
            (true, _) => r.fill(color::selected(k)),
            (false, true) => r.fill(color::checked(k)).hover(color::hover(k)),
            (false, false) => r.hover(color::hover(k)),
        };
        if self.dim { r.dim(motion::DIM) } else { r }
    }
}

/// A list with nothing in it: what is missing and what to do about it.
pub fn empty(k: &Kit, title: impl Into<String>, detail: impl Into<String>) -> El {
    col().w_full().py(space::S6).gap(space::S1).items_center().child(text::name(k, title)).child(text::hint(k, detail).center())
}

/// `empty` with a button under the detail that does something about it (Install, Retry).
pub fn empty_action(k: &Kit, title: impl Into<String>, detail: impl Into<String>, action: El) -> El {
    empty(k, title, detail).child(row().pt(space::S2).child(action))
}

/// Something under way: a spinner and what is happening, in the Label style but wrapping; the
/// spinner sits on the first line. Never a spinner alone.
pub fn loading(k: &Kit, s: impl Into<String>) -> El {
    let line = rune::text(s.into()).font(ty::label(k)).color(color::text_muted(k)).wrap().shrink();
    row().gap(space::S2).items_start().child(Spinner::new().size(size::ICON).color(color::text_muted(k)).build(k)).child(line)
}
