//! What a write will do, as cards for a dialog's body: the changes, what is skipped and why,
//! heads-ups, while it is computed, and when it fails.

use rune::El;
use rune_kit::Kit;

use super::{Card, Notice, Tone, color, item_row, loading, text, ty};

enum Entry {
    /// A subject and its lines, shown as an item row with the lines as a hint.
    Group(String, Vec<String>),
    Line(String),
    /// A muted remark in place of changes, one Label line that wraps.
    Note(String),
}

/// A plan as the caller words it. `build` gives, in order: an optional warning notice, then a
/// card labelled with the heading holding the groups, lines and notes in the order given (then a
/// loading line while the plan is computed), a card "Skipped (n)" and a card "Heads-up", each
/// left out when empty. A failure (`failed`) replaces the cards with one bad notice keyed
/// `<key>-failed`.
///
/// Keys: `<key>-changes`, `<key>-group-<i>`, `<key>-line-<i>`, `<key>-note-<i>`, `<key>-waiting`,
/// `<key>-skipped`, `<key>-heads-up`, `<key>-failed`.
pub struct Plan {
    key: String,
    heading: Option<String>,
    entries: Vec<Entry>,
    skipped: Vec<String>,
    heads_up: Vec<String>,
    notice: Option<El>,
    waiting: Option<String>,
    failed: Option<String>,
}

impl Plan {
    pub fn new(key: impl Into<String>) -> Self {
        Plan { key: key.into(), heading: None, entries: Vec::new(), skipped: Vec::new(), heads_up: Vec::new(), notice: None, waiting: None, failed: None }
    }

    /// The changes card's label, as the caller words it ("18 changes to 6 skills", "Changes").
    pub fn heading(mut self, s: impl Into<String>) -> Self {
        self.heading = Some(s.into());
        self
    }

    /// A subject (a skill) and what happens to it: an item row, Name over the lines joined with
    /// " · " as a hint.
    pub fn group(mut self, subject: impl Into<String>, lines: Vec<String>) -> Self {
        self.entries.push(Entry::Group(subject.into(), lines));
        self
    }

    /// A plain change, one Body row ("Edit plata in Cursor", "Everything is in sync.").
    pub fn line(mut self, s: impl Into<String>) -> Self {
        self.entries.push(Entry::Line(s.into()));
        self
    }

    /// A muted remark in the changes card, in the Label style, wrapping: "Nothing to do.",
    /// "Pick the copy to copy from.", why the plan can't be asked for yet.
    pub fn note(mut self, s: impl Into<String>) -> Self {
        self.entries.push(Entry::Note(s.into()));
        self
    }

    /// What won't be touched and why, one warning line each; no card when empty.
    pub fn skipped(mut self, items: Vec<String>) -> Self {
        self.skipped = items;
        self
    }

    /// Things to know before applying, one info line each; no card when empty.
    pub fn heads_up(mut self, items: Vec<String>) -> Self {
        self.heads_up = items;
        self
    }

    /// A notice on top of the cards (a `Tone::Warn` `Notice`, built and keyed by the caller).
    pub fn notice(mut self, n: El) -> Self {
        self.notice = Some(n);
        self
    }

    /// The plan is being computed: a loading line in the changes card, after any entries.
    pub fn waiting(mut self, text: impl Into<String>) -> Self {
        self.waiting = Some(text.into());
        self
    }

    /// The plan could not be made: a `Tone::Bad` notice instead of the cards.
    pub fn failed(mut self, why: impl Into<String>) -> Self {
        self.failed = Some(why.into());
        self
    }

    /// The blocks for `DialogFrame::body`, top to bottom. The changes card is drawn only when it
    /// has a group, a line, a note or a loading line: a heading alone draws nothing.
    pub fn build(self, k: &Kit) -> Vec<El> {
        let key = self.key;
        let mut out: Vec<El> = self.notice.into_iter().collect();
        if let Some(why) = self.failed {
            out.push(Notice::new(Tone::Bad, why).key(format!("{key}-failed")).build(k).key(format!("{key}-failed")));
            return out;
        }

        let mut changes = Card::new(format!("{key}-changes"));
        if let Some(h) = self.heading {
            changes = changes.label(h);
        }
        let (mut groups, mut lines, mut notes) = (0, 0, 0);
        for entry in self.entries {
            changes = changes.row(match entry {
                Entry::Group(subject, parts) => {
                    groups += 1;
                    item_row(text::name(k, subject), Some(text::hint(k, parts.join(" · "))), None).key(format!("{key}-group-{}", groups - 1))
                }
                Entry::Line(s) => {
                    lines += 1;
                    text::body(k, s).key(format!("{key}-line-{}", lines - 1))
                }
                Entry::Note(s) => {
                    notes += 1;
                    rune::text(s).font(ty::label(k)).color(color::text_muted(k)).wrap().key(format!("{key}-note-{}", notes - 1))
                }
            });
        }
        if let Some(w) = self.waiting {
            changes = changes.row(loading(k, w).key(format!("{key}-waiting")));
        }
        out.extend(changes.build(k));

        let n = self.skipped.len();
        let skipped = Card::new(format!("{key}-skipped")).label(format!("Skipped ({n})")).rows(self.skipped.into_iter().map(|s| Notice::new(Tone::Warn, s).build(k)));
        out.extend(skipped.build(k));
        let heads_up = Card::new(format!("{key}-heads-up")).label("Heads-up").rows(self.heads_up.into_iter().map(|s| Notice::new(Tone::Info, s).build(k)));
        out.extend(heads_up.build(k));
        out
    }
}
