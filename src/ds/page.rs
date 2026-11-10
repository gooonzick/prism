//! The list page: filter, status line, a scrolling list of rows (or an empty state), all on one
//! content edge. Plus the row that goes in the list.

use rune::{El, Icon, ScrollSpec, col, icon, row};
use rune_kit::Kit;

use super::{color, motion, radius, size, space, text};

/// A list page. The filter, the count and the notices stay put; the rows scroll and take the
/// remaining height.
pub struct Page {
    key: String,
    filter: Option<El>,
    count: Option<String>,
    notices: Vec<El>,
    rows: Vec<El>,
    empty: Option<El>,
}

impl Page {
    pub fn new(key: impl Into<String>) -> Self {
        Page { key: key.into(), filter: None, count: None, notices: Vec::new(), rows: Vec::new(), empty: None }
    }

    pub fn filter(mut self, e: El) -> Self {
        self.filter = Some(e);
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
        if self.count.is_some() || !self.notices.is_empty() {
            let mut status = col().w_full().gap(space::S2);
            if let Some(c) = self.count {
                status = status.child(text::label(k, c).key("count"));
            }
            for n in self.notices {
                status = status.child(n);
            }
            page = page.child(status);
        }
        if self.rows.is_empty() {
            return match self.empty {
                Some(e) => page.child(e),
                None => page,
            };
        }
        // Rows paint a highlight `S2` past the content edge. The scroll area clips, so it is
        // `S2` wider than the content on each side and pads that back; the rows own their edge.
        let list = col()
            .key("list")
            .w_full()
            .min_h(0.0)
            .grow(1.0)
            .mx(-space::S2)
            .px(space::S2)
            .items_stretch()
            .gap(space::ROW_GAP)
            .scroll(ScrollSpec { stick_bottom: false })
            .children(self.rows);
        page.child(list)
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
}

impl ListRow {
    pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
        ListRow { key: key.into(), name: name.into(), icon: None, meta: None, tags: Vec::new(), selected: false, dim: false, reduced: false }
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
        let height = if self.meta.is_some() { size::ROW_2 } else { size::ROW_1 };
        let mut body = col().grow(1.0).min_w(size::ROW_NAME_MIN).child(text::name(k, self.name.clone()).key(format!("{}-name", self.key)));
        if let Some(m) = self.meta {
            body = body.child(text::label(k, m));
        }
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
            .press_scale(motion::press_scale(self.reduced))
            .on_press(on_press);
        if let Some(i) = self.icon {
            r = r.child(icon(i, size::ICON).color(color::text_muted(k)));
        }
        r = r.child(body);
        if !self.tags.is_empty() {
            // Tags give way to the name: past `ROW_NAME_MIN` they narrow and are cut at the end.
            r = r.child(row().shrink().gap(space::S1).center_y().clip_rounded(radius::TAG).children(self.tags));
        }
        r = if self.selected { r.fill(color::selected(k)) } else { r.hover(color::hover(k)) };
        if self.dim { r.dim(motion::DIM) } else { r }
    }
}

/// A list with nothing in it: what is missing and what to do about it.
pub fn empty(k: &Kit, title: impl Into<String>, detail: impl Into<String>) -> El {
    col().w_full().py(space::S6).gap(space::S1).items_center().child(text::name(k, title)).child(text::hint(k, detail).center())
}
