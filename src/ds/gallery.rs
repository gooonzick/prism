//! Every block once, on one screen (`--scene ds`), so the system can be reviewed in both themes
//! before the screens use it. Generic over the app's state; every handler does nothing.

use rune::{El, TextEdit, Tx, col, row};
use rune_kit::{Kit, glyphs};

use super::{
    Card, Inspector, ListRow, LoadingButton, MenuAction, Notice, Page, Place, Plan, SecretInput, Selection, Tone, button, color, copy_button, danger, dot, empty,
    empty_action, field, field_pair, filter, icon_button, item_row, loading, markdown, overflow, pair_row, plain_row, pressable, primary,
    quiet, reveal, segments, size, space, stacked_row, status, switch, tabs, tag, text,
};

const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGq0e6m3cX9y5Vb8nqE2Yt7kU1dJw4LrP0sZf9hQ3xTa work@laptop";

fn nothing<S>() -> impl Fn(&mut S) + Clone + 'static {
    |_: &mut S| {}
}

/// A labelled group of the gallery.
fn section(k: &Kit, name: &str, body: El) -> El {
    col().w_full().gap(space::S2).child(text::label(k, name)).child(body)
}

pub fn gallery<S: 'static>(k: &Kit, layer: &mut Vec<El>) -> El {
    let blocks = col()
        .w(300.0)
        .h_full()
        .min_h(0.0)
        .padding(size::PANE_EDGE)
        .gap(space::S6)
        .child(section(k, "Type", type_ladder(k)))
        .child(section(k, "Status", statuses(k)))
        .child(section(k, "Notices", notices(k)))
        .child(section(k, "Controls", controls::<S>(k, layer)))
        .child(section(k, "Selection", summary_line::<S>(k)));
    let cards = col()
        .w(320.0)
        .h_full()
        .min_h(0.0)
        .padding(size::PANE_EDGE)
        .gap(space::S4)
        .children(cards::<S>(k).into_iter().flatten());
    col().key("ds-gallery").w_full().h_full().fill(color::pane(k)).child(
        row().w_full().h_full().items_start().child(blocks).child(cards).child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .child(col().w_full().grow(1.0).min_h(0.0).child(page::<S>(k)))
                .child(col().w_full().px(size::PANE_EDGE).pb(size::PANE_EDGE).child(section(k, "States", states(k)))),
        ).child(
            col()
                .w(size::INSPECTOR_MAX)
                .h_full()
                .border(1.0, color::hairline(k))
                .child(inspector::<S>(k, layer))
                .child(col().w_full().grow(1.0).min_h(0.0).child(picking::<S>(k))),
        ).child(
            col()
                .w(320.0)
                .h_full()
                .min_h(0.0)
                .padding(size::PANE_EDGE)
                .gap(space::S4)
                .child(section(k, "Tabs", row().child(tabs(k, "Show", &[("gallery-tab-skills", "Skills"), ("gallery-tab-mcp", "MCP servers")], 0, |_: &mut S, _, _: &mut Tx| {}))))
                .child(section(k, "Plan", col().w_full().gap(space::S4).children(plan(k))))
                .child(section(k, "Command", text::command(k, "npx -y figma-developer-mcp --stdio --figma-api-key=${FIGMA_KEY}")))
                .child(section(k, "Markdown", markdown::<S>(k, MARKDOWN)))
                .child(section(k, "Inspector controls", inspector_controls::<S>(k))),
        ),
    )
}

const MARKDOWN: &str = "# find-skills\n\nFinds a skill on **skills.sh**:\n\n- ask for one by task\n\n```\nnpx skills add owner/repo\n```";

/// A plan as a dialog shows it: grouped changes, what is skipped, a heads-up.
fn plan(k: &Kit) -> Vec<El> {
    Plan::new("gallery-plan")
        .heading("3 changes to 2 hosts")
        .group("work", vec!["add IdentityFile".into(), "set Port 2222".into()])
        .group("github", vec!["remove ForwardAgent".into()])
        .skipped(vec!["pi: the file is read-only".into()])
        .heads_up(vec!["work is open in another window".into()])
        .build(k)
}

/// A switch off and on, the reveal button in both states, a secret field hidden and shown.
fn inspector_controls<S: 'static>(k: &Kit) -> El {
    // Like the gallery's other fields, edits are not wired.
    let secret = |shown: bool| {
        let edit = TextEdit::new("hunter2-fake-token").secure();
        SecretInput::unwired(&edit, shown, nothing::<S>()).label("Value").build(k)
    };
    col()
        .w_full()
        .gap(space::S2)
        .child(
            row()
                .gap(space::S3)
                .center_y()
                .child(switch(k, false).label("Off").hide_label().build(k))
                .child(switch(k, true).label("On").hide_label().build(k)),
        )
        .child(row().gap(space::S2).center_y().child(reveal(false, Place::Inspector).build(k)).child(reveal(true, Place::Inspector).build(k)))
        .child(secret(false))
        .child(secret(true))
}

fn type_ladder(k: &Kit) -> El {
    col()
        .w_full()
        .gap(space::S1)
        .child(text::title(k, "Title: work.example.com"))
        .child(text::value(k, "Value: deploy@10.0.0.5"))
        .child(text::name(k, "Name: work"))
        .child(text::code(k, "Code: SHA256:q8Jt0nYxk2c"))
        .child(text::path(k, "Path: ~/.ssh/config.d/a/very/long/folder/name/work.conf"))
        .child(text::label(k, "Label: Port"))
        .child(text::hint(k, "Hint: used when no other key matches"))
        .child(text::body(k, "Body: a sentence that wraps onto a second line when the column is narrow."))
}

fn statuses(k: &Kit) -> El {
    let dots = row().gap(space::S2).center_y().children([Tone::Neutral, Tone::Ok, Tone::Warn, Tone::Bad, Tone::Info].map(|t| dot(k, t)));
    col()
        .w_full()
        .gap(space::S2)
        .child(dots)
        .child(row().gap(space::S3).child(status(k, Tone::Ok, "In the agent")).child(status(k, Tone::Bad, "Missing")))
        .child(row().gap(space::S1).child(tag(k, "ed25519", None, "Key type")).child(tag(k, "agent", Some(Tone::Ok), "In the agent")).child(tag(k, "pattern", Some(Tone::Info), "Matches several hosts")))
}

fn notices(k: &Kit) -> El {
    col()
        .w_full()
        .gap(space::S2)
        .child(Notice::new(Tone::Warn, "ssh-agent is not running").detail("Keys can't be added until it starts").build(k))
        .child(Notice::new(Tone::Bad, "Permissions are 644: others can read this key").action(button("Fix", Place::Inspector).build(k)).build(k))
}

fn controls<S: 'static>(k: &Kit, layer: &mut Vec<El>) -> El {
    let edit = TextEdit::new("");
    let more = overflow::<S>(k, "gallery-more", Place::List, false, nothing(), nothing(), vec![MenuAction::new("Move up", nothing())], layer);
    col()
        .w_full()
        .gap(space::S2)
        .child(
            row()
                .w_full()
                .gap(space::S2)
                .center_y()
                .flex_wrap()
                .child(primary("Add host", Place::Bar).build(k))
                .child(button("Edit", Place::Bar).build(k))
                .child(quiet("Fewer", Place::Bar).build(k))
                .child(danger("Remove…", Place::Bar).build(k))
                .child(primary("Applying…", Place::Bar).loading(true).build(k)),
        )
        .child(
            row()
                .gap(space::S2)
                .center_y()
                .child(copy_button("Copy", Place::Inspector, false).build(k))
                .child(copy_button("Copied", Place::Inspector, true).build(k))
                .child(icon_button(&glyphs::SEARCH, "Search", Place::List).build(k))
                .child(more),
        )
        .child(filter(&edit, "Filter hosts").build(k))
}

fn cards<S: 'static>(k: &Kit) -> Vec<Option<El>> {
    let input = |s: &str| text::value(k, s.to_string());
    vec![
        Card::new("gallery-pairs")
            .label("Connection")
            .row(pair_row(k, "Address", text::value(k, "10.0.0.5")))
            .row(pair_row(k, "User", text::value(k, "deploy")))
            .row(pair_row(k, "Jump through", text::value(k, "bastion.example.com.a-very-long-domain.internal")))
            .build(k),
        Card::new("gallery-stacked")
            .label("Public key")
            .trailing(copy_button("Copy", Place::Inspector, false).build(k))
            .row(stacked_row(k, "ed25519", text::code(k, KEY)))
            .build(k),
        Card::new("gallery-items")
            .label("Keys")
            .row(item_row(text::name(k, "id_work"), Some(text::hint(k, "ed25519 · in the agent")), Some(quiet("Show", Place::Inspector).build(k))))
            .row(pressable::<S>(k, item_row(text::name(k, "github"), Some(text::hint(k, "Pressable row")), None), "github", false, nothing()))
            .build(k),
        Card::new("gallery-fields")
            .label("Form")
            .row(field(k, "Name", input("work")))
            .row(field_pair(field(k, "Address", input("10.0.0.5")), field(k, "Port", input("22")), Some(size::FIELD_SHORT)))
            .row(plain_row(row().gap(space::S2).child(button("Add option", Place::Inspector).build(k))))
            .build(k),
    ]
}

fn page<S: 'static>(k: &Kit) -> El {
    let edit = TextEdit::new("");
    Page::new("gallery-page")
        .filter(filter(&edit, "Filter hosts").build(k))
        .count("5 hosts")
        .notice(Notice::new(Tone::Info, "~/.ssh/config.d/old.conf could not be read").build(k))
        .rows([
            ListRow::new("gallery-row1", "work").meta("deploy@10.0.0.5").tags([tag(k, "id_work", None, "Key")]).selected(true).build(k, nothing::<S>()),
            ListRow::new("gallery-row2", "github").meta("git@github.com").build(k, nothing::<S>()),
            ListRow::new("gallery-row3", "pi").build(k, nothing::<S>()),
            ListRow::new("gallery-row4", "*.internal").meta("pattern").dim(true).build(k, nothing::<S>()),
            ListRow::new("gallery-row5", "id_old").icon(&glyphs::FILE_TEXT).build(k, nothing::<S>()),
        ])
        .build(k)
        .child(empty(k, "No hosts match", "Clear the filter to see every host"))
}

fn inspector<S: 'static>(k: &Kit, layer: &mut Vec<El>) -> El {
    let more = overflow::<S>(k, "gallery-inspector-more", Place::Inspector, false, nothing(), nothing(), vec![MenuAction::new("Move up", nothing())], layer);
    Inspector::new()
        .subject("gallery")
        .title("work")
        .meta("~/.ssh/config · line 12")
        .status(status(k, Tone::Ok, "Reachable"))
        .description("Work box behind the office VPN.")
        .actions(
            vec![button("Edit", Place::Inspector).build(k), button("Duplicate", Place::Inspector).build(k), more],
            Some(danger("Remove…", Place::Inspector).build(k)),
        )
        .notice(Notice::new(Tone::Warn, "IdentityFile ~/.ssh/id_old is missing").build(k))
        .card(Card::new("gallery-inspector-card").label("Options").row(pair_row(k, "ServerAliveInterval", text::value(k, "30"))).build(k))
        .build(k)
}

/// The selection line while nothing is picked: summary and an action.
fn summary_line<S: 'static>(k: &Kit) -> El {
    Selection::new(false, false, true, nothing::<S>())
        .label("Select all shown")
        .summary("5 hosts in ~/.ssh/config")
        .action(quiet("Clean up 1", Place::List).build(k))
        .build(k)
}

fn states(k: &Kit) -> El {
    col()
        .w_full()
        .gap(space::S2)
        .child(loading(k, "Connecting…"))
        .child(empty_action(k, "Can't reach work", "Connection timed out", button("Retry", Place::List).build(k)))
}

/// A page whose rows can be picked: segments, the selection line with rows picked (the bulk
/// bar), picked and selected rows with a trailing status, and a footer.
fn picking<S: 'static>(k: &Kit) -> El {
    let labels = ["All 5", "Failing 1", "Patterns 1"].map(String::from);
    let pick = |key: &str, name: &str, on: bool| {
        ListRow::new(format!("gallery-pick-{key}"), name).pick(on, format!("Select {name}"), |_: &mut S, _: &mut Tx| {}).pick_key(format!("gallery-check-{key}"))
    };
    let footer = col()
        .w_full()
        .gap(space::S2)
        .child(text::label(k, "Or add hosts from another config file"))
        .child(row().gap(space::S2).child(button("Choose file…", Place::List).build(k)).child(quiet("Import all", Place::List).build(k)));
    Page::new("gallery-picking")
        .segments(segments(&labels, Some(0), "Show", |_: &mut S, _, _: &mut Tx| {}).build(k))
        .selection(
            Selection::new(false, true, true, nothing::<S>())
                .picked(2, nothing::<S>())
                .action(button("Test", Place::List).build(k))
                .action(button("Copy to…", Place::List).build(k))
                .danger(danger("Remove…", Place::List).build(k))
                .build(k),
        )
        .rows([
            pick("work", "work", true).meta("Picked").trailing(status(k, Tone::Ok, "Reachable")).build(k, nothing::<S>()),
            pick("github", "github", true).meta("Picked and selected").selected(true).trailing(status(k, Tone::Ok, "Reachable")).build(k, nothing::<S>()),
            pick("pi", "pi", false).trailing(status(k, Tone::Bad, "Timed out")).build(k, nothing::<S>()),
        ])
        .footer(footer)
        .build(k)
}
