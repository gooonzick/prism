//! Status blocks: the dot, the dot plus a word, tags and notices. Status colour appears only on
//! the dot; every word is ordinary text.

use rune::{El, col, el, row};
use rune_kit::Kit;

use super::{color, radius, size, space, text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Ok,
    Warn,
    Bad,
    Info,
}

fn tone_color(k: &Kit, tone: Tone) -> rune::Color {
    match tone {
        Tone::Neutral => color::text_faint(k),
        Tone::Ok => color::ok(k),
        Tone::Warn => color::warn(k),
        Tone::Bad => color::bad(k),
        Tone::Info => color::info(k),
    }
}

/// A 7 by 7 round dot in the tone's colour.
pub fn dot(k: &Kit, tone: Tone) -> El {
    el().size(size::DOT, size::DOT).radius(size::DOT / 2.0).fill(tone_color(k, tone))
}

/// A dot followed by a word.
pub fn status(k: &Kit, tone: Tone, word: impl Into<String>) -> El {
    row().gap(space::S2).center_y().child(dot(k, tone)).child(text::label(k, word))
}

/// A small outlined label, with a dot when it signals something; `help` is its tooltip.
pub fn tag(k: &Kit, label: impl Into<String>, tone: Option<Tone>, help: impl Into<String>) -> El {
    let mut c = row().h(20.0).px(6.0).gap(space::S1).center_y().radius(radius::TAG).border(1.0, color::hairline(k)).help(help.into());
    if let Some(tone) = tone {
        c = c.child(dot(k, tone));
    }
    c.child(text::label(k, label))
}

/// A line of news for the status area: a dot, a message that wraps, an optional detail and action.
pub struct Notice {
    tone: Tone,
    text: String,
    detail: Option<String>,
    action: Option<El>,
    key: Option<String>,
}

impl Notice {
    pub fn new(tone: Tone, text: impl Into<String>) -> Self {
        Notice { tone, text: text.into(), detail: None, action: None, key: None }
    }

    pub fn detail(mut self, s: impl Into<String>) -> Self {
        self.detail = Some(s.into());
        self
    }

    /// Keys the message text as `<key>-text`.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn action(mut self, button: El) -> Self {
        self.action = Some(button);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let mut message = text::body(k, self.text);
        if let Some(key) = self.key {
            message = message.key(format!("{key}-text"));
        }
        let mut body = col().grow(1.0).min_w(0.0).gap(2.0).child(message);
        if let Some(d) = self.detail {
            body = body.child(text::hint(k, d));
        }
        let mut line = row().w_full().gap(space::S3).items_start().child(dot(k, self.tone).mt(7.0)).child(body);
        if let Some(a) = self.action {
            line = line.child(a);
        }
        line
    }
}
