//! Documents: a Markdown file (SKILL.md) shown in an inspector, styled on the type roles so the
//! file never outranks the pane around it.

use rune::{El, Tx};
use rune_kit::{Kit, Markdown, MarkdownStyle};

use super::{color, ty};

/// Markdown on the type roles: body text is Body (`ty::value`, wraps), headings 1 to 3 are Name
/// (`ty::name`, never larger than the pane's title), inline and block code are Code, list markers
/// and deeper headings muted. Links open in the browser.
pub fn markdown<S: 'static>(k: &Kit, md: impl Into<String>) -> El {
    let body = ty::value(k);
    let code = ty::code(k);
    let mut style = MarkdownStyle::from_kit(k)
        .text(color::text(k))
        .muted(color::text_muted(k))
        .body_font(body)
        .code_block_font(code)
        .inline_code_font(code.size / body.size, code.weight);
    for level in 1..=3 {
        style = style.heading_font(level, ty::name(k));
    }
    Markdown::new(md)
        .style(style)
        .on_link_tx(|_: &mut S, url: &str, tx: &mut Tx| {
            let _ = tx.open_url(url);
        })
        .build(k)
}
