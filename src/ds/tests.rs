use super::*;
use rune_kit::Kit;

#[test]
fn scales() {
    assert_eq!([space::S1, space::S2, space::S3, space::S4, space::S6], [4.0, 8.0, 12.0, 16.0, 24.0]);
    assert_eq!([radius::TAG, radius::CONTROL, radius::CARD, radius::DIALOG], [4.0, 6.0, 8.0, 12.0]);
}

#[test]
fn reduced_motion_keeps_only_opacity() {
    assert_eq!(motion::press_scale(true), 1.0);
    assert_eq!(motion::press_scale(false), 0.97);
}

#[test]
fn type_ladder() {
    let k = Kit::neutral(false);
    let t = ty::title(&k);
    assert_eq!((t.size, t.weight, t.line_height), (15.0, 600.0, 20.0));
    let v = ty::value(&k);
    assert_eq!((v.size, v.weight, v.line_height), (13.0, 400.0, 20.0));
    assert_eq!(ty::name(&k).weight, 500.0);
    let c = ty::code(&k);
    assert_eq!((c.size, c.line_height), (12.0, 18.0));
    assert_eq!(c.family, k.ty.mono.family);
    let l = ty::label(&k);
    assert_eq!((l.size, l.weight, l.line_height), (11.0, 400.0, 16.0));
    assert_eq!(ty::hint(&k), l);
}

#[test]
fn roles_differ_in_both_themes() {
    for dark in [false, true] {
        let k = Kit::neutral(dark);
        assert_ne!(color::text(&k), color::text_muted(&k));
        assert_ne!(color::text_muted(&k), color::text_faint(&k));
        assert_ne!(color::text(&k), color::text_faint(&k));
        assert_ne!(color::card(&k), color::selected(&k));
    }
}

#[test]
fn places() {
    use rune::{Env, Runtime, row};
    fn view(_: &(), _: &mut rune::Cx<()>) -> rune::El {
        let k = Kit::neutral(false);
        let place = |p: Place, key: &str| button("Edit", p).build(&k).key(key.to_string());
        row().child(place(Place::Inspector, "inspector")).child(place(Place::List, "list")).child(place(Place::Bar, "bar"))
    }
    let mut rt = Runtime::new((), view, (400.0, 100.0), Env { system_dark: false, scale: 1.0 });
    rt.frame(0.0);
    let h = |key: &str| rt.presented(key).unwrap_or_else(|| panic!("no {key}")).0[3];
    // The spec's 32 for bars does not exist in rune-kit; bars use 28 (DESIGN.md, exceptions).
    assert_eq!([h("inspector"), h("list"), h("bar")], [24.0, 28.0, 28.0]);
}

#[test]
fn pane_widths() {
    // 352, not the spec's first 320: the Keys action row with "Remove from agent" needs it.
    assert_eq!([size::INSPECTOR_MIN, size::INSPECTOR_MAX, size::LIST_MIN], [352.0, 400.0, 276.0]);
    // The minimum window (860) with the default sidebar (232) shows all three panes.
    const { assert!(232.0 + size::LIST_MIN + size::INSPECTOR_MIN <= 860.0) };
}

mod blocks {
    use super::*;
    use rune::{Cx, El, Env, Runtime, col};

    const W: f32 = 400.0;

    fn rt(view: fn(&(), &mut Cx<()>) -> El) -> Runtime<()> {
        let mut rt = Runtime::new((), view, (W, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    fn long() -> String {
        "x".repeat(300)
    }

    fn shell(card: Option<El>) -> El {
        col().w_full().h_full().padding(16.0).child(card.expect("card has rows"))
    }

    fn three_rows(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        shell(
            Card::new("card")
                .label("Group")
                .row(pair_row_with(text::label(&k, "Port").key("r1-label"), text::value(&k, "22")))
                .row(stacked_row_with(text::label(&k, "Path").key("r2-label"), text::code(&k, "abc")))
                .row(item_row(text::name(&k, "work").key("r3-name"), Some(text::hint(&k, "deploy")), None))
                .build(&k),
        )
    }

    #[test]
    fn card_rows_share_one_edge() {
        let rt = rt(three_rows);
        let x = |key: &str| rt.presented(key).unwrap_or_else(|| panic!("no {key}")).0[0];
        assert_eq!(x("r1-label"), x("r2-label"));
        assert_eq!(x("r2-label"), x("r3-name"));
    }

    fn long_pair(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        shell(Card::new("card").row(pair_row(&k, "Path", text::value(&k, long()).key("val")).key("row")).build(&k))
    }

    #[test]
    fn pair_row_is_one_line_and_truncates() {
        let rt = rt(long_pair);
        let row = rt.presented("row").unwrap().0;
        assert!(row[3] < 40.0, "row is {} high", row[3]);
        let card = rt.presented("card").unwrap().0;
        assert!(card[2] <= W, "card is {} wide", card[2]);
        let v = rt.presented("val").unwrap().0;
        assert!(v[0] + v[2] <= card[0] + card[2], "value ends at {}, card at {}", v[0] + v[2], card[0] + card[2]);
    }

    fn long_stacked(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        shell(Card::new("card").row(stacked_row(&k, "Key", text::code(&k, long()).key("val")).key("row")).build(&k))
    }

    #[test]
    fn stacked_row_wraps_long_value() {
        let rt = rt(long_stacked);
        let row = rt.presented("row").unwrap().0;
        assert!(row[3] > 40.0, "row is {} high", row[3]);
        let card = rt.presented("card").unwrap().0;
        assert!(card[2] <= W, "card is {} wide", card[2]);
        let v = rt.presented("val").unwrap().0;
        assert!(v[0] + v[2] <= card[0] + card[2], "value ends at {}, card at {}", v[0] + v[2], card[0] + card[2]);
    }

    /// Value rows at the edge of the budget, in a card as wide as the narrowest inspector's.
    fn budget_rows(_: &(), _: &mut Cx<()>) -> El {
        use super::card::{PAIR_LABEL_CHARS, PAIR_LINE_CHARS};
        let k = Kit::neutral(false);
        // A long label with the longest value that still fits, a short one likewise, then one
        // character more.
        let long_label = "L".repeat(PAIR_LINE_CHARS - 6);
        let rows = [(long_label.clone(), "v".repeat(6)), ("Port".to_string(), "v".repeat(PAIR_LINE_CHARS - PAIR_LABEL_CHARS)), (long_label, "v".repeat(7))];
        let card = Card::new("card").rows(rows.into_iter().enumerate().map(|(i, (l, v))| {
            value_row(text::code(&k, l.clone()).truncate().key(format!("l{i}")), &l, text::code(&k, v.clone()).key(format!("v{i}")), &v).key(format!("row{i}"))
        }));
        col().w(size::INSPECTOR_MIN).h_full().padding(size::PANE_EDGE).child(card.build(&k).expect("rows"))
    }

    #[test]
    fn pair_rows_show_the_label_in_full_or_stack() {
        let rt = rt(budget_rows);
        let k = Kit::neutral(false);
        let r = |key: &str| rt.presented(key).unwrap_or_else(|| panic!("no {key}")).0;
        let card = r("card");
        let one_line = 20.0;
        for i in 0..2 {
            let (l, v) = (r(&format!("l{i}")), r(&format!("v{i}")));
            assert!(r(&format!("row{i}"))[3] <= one_line, "row {i} is one line");
            let text = rt.elements().into_iter().find(|e| e.key.as_deref() == Some(&format!("l{i}"))).and_then(|e| e.text).unwrap();
            assert!(l[2] >= rune::text::advance(&text, ty::code(&k)), "label {i} is cut: {l:?}");
            assert!(v[0] + v[2] <= card[0] + card[2], "value {i} ends at {}, the card at {}", v[0] + v[2], card[0] + card[2]);
        }
        assert!(r("row2")[3] > one_line, "one character more and the value goes under the label");
    }

    #[test]
    fn empty_card_is_none() {
        let k = Kit::neutral(false);
        assert!(Card::new("c").label("X").build(&k).is_none());
    }

    fn notice(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        col().w_full().h_full().padding(16.0).child(Notice::new(Tone::Warn, "word ".repeat(40)).key("n").action(rune::el().key("fix").size(60.0, 24.0)).build(&k))
    }

    #[test]
    fn notice_wraps_and_keeps_its_button() {
        let rt = rt(notice);
        let t = rt.presented("n-text").unwrap().0;
        assert!(t[3] > 40.0, "notice text is {} high", t[3]);
        let (x, y) = rt.find("fix").expect("fix button");
        assert!(x > 0.0 && x < W && y > 0.0, "button centre at {x},{y}");
        let b = rt.presented("fix").unwrap().0;
        assert!(b[0] + b[2] <= W, "button ends at {}", b[0] + b[2]);
    }
}

mod skeletons {
    use super::*;
    use rune::{Cx, El, Env, Runtime, col};
    use rune_kit::glyphs;

    fn env() -> Env {
        Env { system_dark: false, scale: 1.0 }
    }

    fn page_view(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let edit = rune::TextEdit::new("");
        Page::new("page")
            .filter(filter(&edit, "Filter").build(&k).key("filter"))
            .count("1 host")
            .notice(Notice::new(Tone::Warn, "Careful").key("notice").build(&k).key("notice-box"))
            .rows([ListRow::new("row", "work").build(&k, |_: &mut ()| {})])
            .build(&k)
    }

    #[test]
    fn page_has_one_content_edge() {
        let mut rt = Runtime::new((), page_view, (400.0, 600.0), env());
        rt.frame(0.0);
        for key in ["filter", "count", "notice-box", "row-name"] {
            let x = rt.presented(key).unwrap_or_else(|| panic!("no {key}")).0[0];
            assert!((x - 16.0).abs() <= 0.5, "{key} starts at {x}");
        }
    }

    fn rows_view(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        col()
            .w_full()
            .child(ListRow::new("with", "a").meta("b").build(&k, |_: &mut ()| {}))
            .child(ListRow::new("without", "a").icon(&glyphs::SEARCH).build(&k, |_: &mut ()| {}))
    }

    fn many_tags(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let tags = (0..8).map(|i| tag(&k, format!("id_ed25519_{i}"), None, ""));
        col().w(300.0).items_stretch().child(ListRow::new("row", "production-db").icon(&glyphs::SEARCH).tags(tags).build(&k, |_: &mut ()| {}))
    }

    #[test]
    fn tags_give_way_to_the_name() {
        let mut rt = Runtime::new((), many_tags, (400.0, 600.0), env());
        rt.frame(0.0);
        let name = rt.presented("row-name").unwrap().0;
        assert!(name[2] >= size::ROW_NAME_MIN, "the name keeps {} of {}", name[2], size::ROW_NAME_MIN);
        let row = rt.presented("row").unwrap().0;
        assert!(name[0] + name[2] <= row[0] + row[2], "the name stays in the row");
    }

    #[test]
    fn list_row_heights() {
        let mut rt = Runtime::new((), rows_view, (400.0, 600.0), env());
        rt.frame(0.0);
        assert_eq!(rt.presented("with").unwrap().0[3], 44.0);
        assert_eq!(rt.presented("without").unwrap().0[3], 32.0);
    }

    fn alone(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        col().w_full().padding(16.0).items_stretch().child(ListRow::new("row", "work").build(&k, |_: &mut ()| {}))
    }

    #[test]
    fn list_row_owns_its_edge() {
        let mut rt = Runtime::new((), alone, (400.0, 600.0), env());
        rt.frame(0.0);
        let row = rt.presented("row").unwrap().0;
        assert_eq!((row[0], row[2]), (8.0, 384.0));
        let name = rt.presented("row-name").unwrap().0;
        assert_eq!(name[0], 16.0);
    }

    fn crowded(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let mut page = Page::new("page").count("40 hosts");
        for i in 0..6 {
            page = page.notice(Notice::new(Tone::Info, format!("notice {i} {}", "word ".repeat(12))).key(format!("n{i}")).build(&k));
        }
        page.rows((0..40).map(|i| ListRow::new(format!("row{i}"), format!("host {i}")).build(&k, |_: &mut ()| {}))).build(&k)
    }

    #[test]
    fn many_notices_leave_the_list_scrollable() {
        let mut rt = Runtime::new((), crowded, (400.0, 600.0), env());
        rt.frame(0.0);
        let last = rt.presented("n5-text").unwrap().0;
        assert!(last[1] + last[3] <= 600.0, "last notice ends at {}", last[1] + last[3]);
        assert!(rt.find("row0").is_some(), "first row is not on screen");
        let page = rt.presented("page").unwrap().0;
        assert!(page[3] <= 600.0, "page is {} high", page[3]);
        let list = rt.presented("list").unwrap().0;
        assert!(list[3] > 100.0, "list is only {} high", list[3]);
    }

    #[derive(Default)]
    struct Menus {
        open: bool,
        ran: u32,
    }

    fn actions_view(s: &Menus, _: &mut Cx<Menus>) -> El {
        let k = Kit::neutral(false);
        let mut layer = Vec::new();
        let more = overflow(
            &k,
            "more",
            Place::Inspector,
            s.open,
            |s: &mut Menus| s.open = !s.open,
            |s: &mut Menus| s.open = false,
            vec![MenuAction::new("Copy", |s: &mut Menus| s.ran += 1)],
            &mut layer,
        );
        let pane = Inspector::new()
            .actions(
                vec![
                    button("One", Place::Inspector).build(&k).key("b1"),
                    button("Two", Place::Inspector).build(&k).key("b2"),
                    button("Three", Place::Inspector).build(&k).key("b3"),
                    more,
                ],
                Some(danger("Delete", Place::Inspector).build(&k).key("del")),
            )
            .build(&k);
        rune_kit::root(col().w_full().h_full().child(pane), layer)
    }

    fn click(rt: &mut Runtime<Menus>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    }

    #[test]
    fn inspector_actions_stay_on_one_row() {
        let mut rt = Runtime::new(Menus::default(), actions_view, (320.0, 400.0), env());
        rt.frame(0.0);
        let ys: Vec<f32> = ["b1", "b2", "b3", "more", "del"].iter().map(|k| rt.find(k).unwrap_or_else(|| panic!("no {k}")).1).collect();
        assert!(ys.iter().all(|y| (y - ys[0]).abs() < 0.5), "centres {ys:?}");
        let del = rt.presented("del").unwrap().0;
        assert!((del[0] + del[2] - (320.0 - 16.0)).abs() <= 0.5, "delete ends at {}", del[0] + del[2]);
    }

    #[test]
    fn overflow_opens_and_runs() {
        let mut rt = Runtime::new(Menus::default(), actions_view, (320.0, 400.0), env());
        rt.frame(0.0);
        click(&mut rt, "more");
        assert!(rt.state.open, "trigger did not toggle");
        click(&mut rt, "more-popup-item0");
        assert_eq!(rt.state.ran, 1);
        assert!(!rt.state.open, "choosing an item closes the menu");
    }
}

mod dialog {
    use super::*;
    use rune::{Cx, El, Env, Key, KeyEvent, Mods, Runtime, col, el};

    fn env() -> Env {
        Env { system_dark: false, scale: 1.0 }
    }

    #[derive(Default)]
    struct Shown {
        closed: u32,
    }

    /// A dialog with one body block `body_h` high, a Cancel and an OK button, over an empty window.
    fn frame(kind: DialogKind, body_h: f32) -> El {
        let k = Kit::neutral(false);
        let mut layer = Vec::new();
        DialogFrame::new("d", "Title", kind)
            .body(el().key("block").w_full().h(body_h))
            .footer(button("Cancel", Place::Bar).build(&k).key("cancel"), primary("OK", Place::Bar).build(&k).key("ok"))
            .build(&k, |s: &mut Shown| s.closed += 1, &mut layer);
        rune_kit::root(col().w_full().h_full(), layer)
    }

    fn run(view: fn(&Shown, &mut Cx<Shown>) -> El, size: (f32, f32)) -> Runtime<Shown> {
        let mut rt = Runtime::new(Shown::default(), view, size, env());
        rt.frame(0.0);
        rt.frame(1.0);
        rt
    }

    fn panel(rt: &Runtime<Shown>) -> [f32; 4] {
        rt.presented("d-dialog").expect("no panel").0
    }

    #[test]
    fn dialog_widths() {
        let confirm = run(|_, _| frame(DialogKind::Confirm, 40.0), (1200.0, 760.0));
        assert_eq!(panel(&confirm)[2], 440.0);
        let form = run(|_, _| frame(DialogKind::Form, 40.0), (1200.0, 760.0));
        assert_eq!(panel(&form)[2], 560.0);
    }

    #[test]
    fn dialogs_share_a_top() {
        let short = run(|_, _| frame(DialogKind::Form, 40.0), (1200.0, 760.0));
        let tall = run(|_, _| frame(DialogKind::Form, 480.0), (1200.0, 760.0));
        assert_eq!(panel(&short)[1], 96.0);
        assert_eq!(panel(&tall)[1], 96.0);
        assert!(panel(&tall)[3] > panel(&short)[3] + 400.0, "the tall dialog is not taller");
    }

    #[test]
    fn tall_form_fits_a_small_window() {
        let rt = run(|_, _| frame(DialogKind::Form, 900.0), (860.0, 540.0));
        let p = panel(&rt);
        assert!(p[1] + p[3] <= 540.0 - 16.0, "panel ends at {}", p[1] + p[3]);
        assert!(rt.find("ok").is_some(), "no footer button");
        let ok = rt.presented("ok").unwrap().0;
        assert!(ok[1] >= 0.0 && ok[1] + ok[3] <= 540.0, "footer button at y {} h {}", ok[1], ok[3]);
        let body = rt.presented("d-body").unwrap().0;
        assert!(body[3] < 900.0, "body is {} high, it does not scroll", body[3]);
    }

    fn click(rt: &mut Runtime<Shown>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    }

    #[test]
    fn escape_and_close_button_close() {
        let mut rt = run(|_, _| frame(DialogKind::Confirm, 40.0), (1200.0, 760.0));
        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        rt.frame(0.0);
        assert_eq!(rt.state.closed, 1, "Escape did not close");
        click(&mut rt, "close");
        assert_eq!(rt.state.closed, 2, "the close button did not close");
    }

    #[test]
    fn outside_press_closes() {
        let mut rt = run(|_, _| frame(DialogKind::Confirm, 40.0), (1200.0, 760.0));
        rt.pointer_move(20.0, 20.0);
        rt.pointer_down(20.0, 20.0);
        rt.pointer_up(20.0, 20.0);
        rt.frame(0.0);
        assert_eq!(rt.state.closed, 1);
    }

    fn with_menu(_: &Shown, _: &mut Cx<Shown>) -> El {
        let k = Kit::neutral(false);
        let mut layer = Vec::new();
        DialogFrame::new("d", "Title", DialogKind::Form)
            .body(el().key("block").w_full().h(40.0))
            .menus(vec![el().key("m").size(10.0, 10.0)])
            .build(&k, |_: &mut Shown| {}, &mut layer);
        col().key("layer").w_full().h_full().children(layer)
    }

    #[test]
    fn menu_layer_order() {
        let rt = run(with_menu, (1200.0, 760.0));
        let all = rt.elements();
        assert!(all.iter().any(|e| e.key.as_deref() == Some("d-dialog")), "no dialog");
        assert_eq!(all.last().and_then(|e| e.key.as_deref()), Some("m"));
    }
}

mod motion_and_gallery {
    use super::*;
    use rune::{Cx, El, Env, Runtime, col};

    fn gallery_view(_: &(), cx: &mut Cx<()>) -> El {
        let k = Kit::neutral(cx.system_dark());
        let mut layer = Vec::new();
        let g = gallery::<()>(&k, &mut layer);
        rune_kit::root(g, layer)
    }

    #[test]
    fn gallery_renders_in_both_themes() {
        for dark in [false, true] {
            let mut rt = Runtime::new((), gallery_view, (1200.0, 760.0), Env { system_dark: dark, scale: 1.0 });
            rt.frame(0.0);
            assert!(rt.find("ds-gallery").is_some(), "no gallery (dark: {dark})");
        }
    }

    fn subject_view(s: &u32, _: &mut Cx<u32>) -> El {
        let k = Kit::neutral(false);
        col().w_full().h_full().child(Inspector::new().subject(format!("obj{s}")).title(format!("Object {s}")).build(&k))
    }

    #[test]
    fn inspector_body_fades_when_the_subject_changes() {
        let mut rt = Runtime::new(0u32, subject_view, (400.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        assert_eq!(rt.presented("inspector-obj0").map(|p| p.1), Some(1.0), "the first body shows at once");
        rt.update(|s, _| *s = 1);
        rt.frame(1.0 / 60.0);
        let old = |rt: &Runtime<u32>| rt.elements().into_iter().find(|e| e.key.as_deref() == Some("inspector-obj0"));
        // The old body fades out on the same `swap` spring the new one fades in on, so the two
        // opacities always add up to one.
        for frame in 0..6 {
            let new = rt.presented("inspector-obj1").expect("the new body").1;
            let leaving = old(&rt).unwrap_or_else(|| panic!("the old body is gone at frame {frame}"));
            assert!(leaving.exiting, "the old body is not exiting");
            assert!(new < 1.0 && leaving.opacity > 0.0, "frame {frame}: new {new}, old {}", leaving.opacity);
            assert!((new + leaving.opacity - 1.0).abs() < 0.01, "frame {frame}: new {new} + old {} is not 1", leaving.opacity);
            rt.frame(1.0 / 60.0);
        }
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.presented("inspector-obj1").map(|p| p.1), Some(1.0));
        assert!(old(&rt).is_none(), "the old body is gone");
    }
}

mod page_additions {
    use super::*;
    use rune::{Cx, El, Env, Mods, PhysicalInput, Runtime, TextEdit, Tx, col};

    fn env() -> Env {
        Env { system_dark: false, scale: 1.0 }
    }

    fn run<S: 'static>(state: S, view: fn(&S, &mut Cx<S>) -> El, size: (f32, f32)) -> Runtime<S> {
        let mut rt = Runtime::new(state, view, size, env());
        rt.frame(0.0);
        rt
    }

    fn rect<S: 'static>(rt: &Runtime<S>, key: &str) -> [f32; 4] {
        rt.presented(key).unwrap_or_else(|| panic!("no {key}")).0
    }

    fn click<S: 'static>(rt: &mut Runtime<S>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    }

    fn row(k: &Kit, name: &str) -> El {
        ListRow::new(format!("skill-{name}"), name).build(k, |_: &mut ()| {})
    }

    fn with_segments(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let edit = TextEdit::new("");
        let labels = vec!["All 3".to_string(), "Broken 1".to_string()];
        Page::new("page")
            .filter(filter(&edit, "Filter").build(&k).key("filter"))
            .segments(segments(&labels, Some(0), "Show", |_: &mut (), _, _: &mut Tx| {}).build(&k).key("segments"))
            .count("3 skills")
            .rows([row(&k, "a")])
            .build(&k)
    }

    #[test]
    fn segments_sit_between_filter_and_status() {
        let rt = run((), with_segments, (400.0, 600.0));
        let y = |key: &str| rect(&rt, key)[1];
        assert!(y("filter") < y("segments"), "filter {} segments {}", y("filter"), y("segments"));
        assert!(y("segments") < y("count"), "segments {} count {}", y("segments"), y("count"));
        assert!(y("count") < y("skill-a"), "count {} row {}", y("count"), y("skill-a"));
        assert_eq!(rect(&rt, "segments")[3], 28.0, "segments sit at the list's control height");
    }

    fn picking(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        Page::new("page")
            .selection(Selection::new(false, false, true, |_: &mut ()| {}).summary("2 skills").build(&k))
            .rows(["a", "b"].map(|n| {
                ListRow::new(format!("skill-{n}"), n).pick(n == "b", format!("Select {n}"), |_: &mut (), _: &mut Tx| {}).pick_key(format!("check-{n}")).build(&k, |_: &mut ()| {})
            }))
            .build(&k)
    }

    #[test]
    fn pick_checkbox_lines_up_with_select_all() {
        let rt = run((), picking, (400.0, 600.0));
        let (all, a) = (rect(&rt, "check-all"), rect(&rt, "check-a"));
        assert!((all[0] - a[0]).abs() <= 0.5, "select-all at x {}, row checkbox at x {}", all[0], a[0]);
        assert!((all[2] - a[2]).abs() <= 0.5, "select-all {} wide, row checkbox {}", all[2], a[2]);
    }

    fn fills(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let pick = |key: &str, selected: bool| ListRow::new(key, key).pick(true, "Select", |_: &mut (), _: &mut Tx| {}).selected(selected).build(&k, |_: &mut ()| {});
        col().w_full().padding(16.0).items_stretch().child(pick("picked", false)).child(pick("both", true))
    }

    #[test]
    fn checked_row_fills_checked_selected_wins() {
        let rt = run((), fills, (400.0, 600.0));
        let k = Kit::neutral(false);
        let fill = |key: &str| rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}")).fill;
        assert_eq!(fill("picked"), color::checked(&k));
        assert_eq!(fill("both"), color::selected(&k));
        assert_ne!(color::checked(&k), color::selected(&k));
    }

    fn trailing(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let r = |key: &str, word: &str| ListRow::new(key, key).trailing(status(&k, Tone::Ok, word).key(format!("{key}-word"))).build(&k, |_: &mut ()| {});
        Page::new("page").rows([r("short", "synced"), r("long", "not in .agents")]).build(&k)
    }

    #[test]
    fn trailing_columns_line_up() {
        let rt = run((), trailing, (400.0, 600.0));
        let (a, b) = (rect(&rt, "short-trailing"), rect(&rt, "long-trailing"));
        assert_eq!(a[2], size::ROW_TRAILING);
        assert_eq!(b[2], size::ROW_TRAILING);
        assert!((a[0] + a[2] - (b[0] + b[2])).abs() <= 0.5, "right edges {} and {}", a[0] + a[2], b[0] + b[2]);
        // The words end on the column's right edge.
        let (wa, wb) = (rect(&rt, "short-word"), rect(&rt, "long-word"));
        assert!((wa[0] + wa[2] - (wb[0] + wb[2])).abs() <= 0.5, "words end at {} and {}", wa[0] + wa[2], wb[0] + wb[2]);
        // The column ends on the row's content edge, `S2` inside its highlight.
        let row = rect(&rt, "short");
        assert!((a[0] + a[2] - (row[0] + row[2] - space::S2)).abs() <= 0.5, "the column ends at {}, the row at {}", a[0] + a[2], row[0] + row[2]);
    }

    fn bulk(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let line = Selection::new(false, true, true, |_: &mut ()| {})
            .picked(3, |_: &mut ()| {})
            .action(button("Sync", Place::List).build(&k).key("a1"))
            .action(button("Copy to…", Place::List).build(&k).key("a2"))
            .danger(danger("Remove…", Place::List).build(&k).key("del"))
            .build(&k);
        Page::new("page").selection(line).rows([row(&k, "a")]).build(&k)
    }

    #[test]
    fn selection_bar_keeps_danger_right() {
        let rt = run((), bulk, (640.0, 400.0));
        let del = rect(&rt, "del");
        assert!((del[0] + del[2] - (640.0 - 16.0)).abs() <= 0.5, "danger ends at {}", del[0] + del[2]);
        assert!(rt.find("bulk-bar").is_some() && rt.find("bulk-clear").is_some(), "no bulk bar");
        let narrow = run((), bulk, (400.0, 400.0));
        for key in ["check-all", "bulk-bar", "bulk-clear", "a1", "a2", "del"] {
            let r = rect(&narrow, key);
            assert!(r[0] >= 16.0 - 0.5 && r[0] + r[2] <= 400.0 - 16.0 + 0.5, "{key} spans {}..{}", r[0], r[0] + r[2]);
        }
        let row = rect(&narrow, "skill-a");
        let del = rect(&narrow, "del");
        assert!(del[1] + del[3] <= row[1], "the bar ends at {}, the first row starts at {}", del[1] + del[3], row[1]);
    }

    fn footer(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let foot = col().key("footer").w_full().gap(space::S2).child(text::label(&k, "Or install straight from a repository")).child(button("List", Place::List).build(&k));
        Page::new("page").rows((0..50).map(|i| row(&k, &format!("r{i}")))).footer(foot).build(&k)
    }

    #[test]
    fn footer_does_not_scroll() {
        let rt = run((), footer, (400.0, 400.0));
        let f = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("footer")).expect("no footer");
        assert_eq!(f.visible, f.rect, "the footer is cut: {:?} of {:?}", f.visible, f.rect);
        assert!((f.rect[1] + f.rect[3] - (400.0 - 16.0)).abs() <= 0.5, "the footer ends at {}", f.rect[1] + f.rect[3]);
        let list = rect(&rt, "list");
        assert!(list[1] + list[3] <= f.rect[1], "the list ends at {}, under the footer at {}", list[1] + list[3], f.rect[1]);
        assert!(list[3] > 100.0, "the list is only {} high", list[3]);
    }

    #[derive(Default)]
    struct Seen {
        cmd: Option<bool>,
        rows: u32,
        ticks: u32,
    }

    fn pressed(_: &Seen, _: &mut Cx<Seen>) -> El {
        let k = Kit::neutral(false);
        let r = ListRow::new("row", "a")
            .pick(false, "Select a", |s: &mut Seen, _: &mut Tx| s.ticks += 1)
            .pick_key("check-a")
            .build_tx(&k, |s: &mut Seen, tx: &mut Tx| {
                s.rows += 1;
                s.cmd = Some(tx.mods().cmd);
            });
        col().w_full().padding(16.0).items_stretch().child(r)
    }

    #[test]
    fn build_tx_sees_modifiers() {
        let mut rt = run(Seen::default(), pressed, (400.0, 600.0));
        rt.physical_input(PhysicalInput::Modifiers(Mods { cmd: true, ..Mods::default() }));
        click(&mut rt, "row-name");
        assert_eq!(rt.state.cmd, Some(true), "the handler did not see ⌘");
    }

    #[test]
    fn pick_toggles_without_pressing_the_row() {
        let mut rt = run(Seen::default(), pressed, (400.0, 600.0));
        click(&mut rt, "check-a");
        assert_eq!((rt.state.ticks, rt.state.rows), (1, 0));
    }

    fn states(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        // The runtime sizes the root to the window, so the narrow column goes one level down.
        col().w_full().child(
            col()
                .w(160.0)
                .child(loading(&k, "Reading the repository…").key("short"))
            .child(loading(&k, "Installing find-skills into ~/.agents/skills on work…").key("long"))
                .child(empty_action(&k, "Can't reach work", "Timed out", button("Retry", Place::List).build(&k).key("retry")).key("empty")),
        )
    }

    #[test]
    fn loading_wraps_and_empty_keeps_its_action() {
        let rt = run((), states, (400.0, 600.0));
        let line = ty::label(&Kit::neutral(false)).line_height;
        assert!(rect(&rt, "short")[3] <= line + 0.5, "a short loading line is {} high", rect(&rt, "short")[3]);
        let long = rect(&rt, "long");
        assert!(long[3] > line + 0.5, "the long loading text did not wrap: {} high", long[3]);
        assert!(long[0] + long[2] <= 160.0 + 0.5, "the long loading text ends at {}", long[0] + long[2]);
        let text = rt.elements().into_iter().find(|e| e.text.as_deref().is_some_and(|t| t.starts_with("Installing"))).expect("no loading text");
        assert!(text.rect[0] + text.rect[2] <= 160.0 + 0.5, "the text ends at {}", text.rect[0] + text.rect[2]);
        let (e, b) = (rect(&rt, "empty"), rect(&rt, "retry"));
        assert!(b[1] >= e[1] && b[1] + b[3] <= e[1] + e[3], "the button sits in the empty state");
    }

    fn edges(_: &(), _: &mut Cx<()>) -> El {
        let k = Kit::neutral(false);
        let edit = TextEdit::new("");
        Page::new("page")
            .filter(filter(&edit, "Filter").build(&k).key("filter"))
            .rows([ListRow::new("row", "work").trailing(status(&k, Tone::Ok, "Reachable")).build(&k, |_: &mut ()| {})])
            .build(&k)
    }

    #[test]
    fn trailing_ends_on_the_filter_edge() {
        let rt = run((), edges, (400.0, 600.0));
        let (t, f) = (rect(&rt, "row-trailing"), rect(&rt, "filter"));
        assert!((t[0] + t[2] - (f[0] + f[2])).abs() <= 0.5, "the trailing column ends at {}, the filter at {}", t[0] + t[2], f[0] + f[2]);
        // The highlight reaches `S2` past the content edge on both sides.
        let row = rect(&rt, "row");
        assert!((row[0] - (f[0] - space::S2)).abs() <= 0.5 && (row[0] + row[2] - (f[0] + f[2] + space::S2)).abs() <= 0.5, "row spans {}..{}", row[0], row[0] + row[2]);
    }

    #[derive(Default)]
    struct Toggles {
        all: u32,
        cleared: u32,
    }

    fn line(enabled: bool, partial: bool, picked: bool) -> El {
        let k = Kit::neutral(false);
        let mut sel = Selection::new(false, partial, enabled, |s: &mut Toggles| s.all += 1).summary("3 hosts");
        if picked {
            sel = sel.picked(3, |s: &mut Toggles| s.cleared += 1).clear_help("Clear selection (Esc)");
        }
        col().w_full().padding(16.0).child(sel.build(&k))
    }

    fn find_key<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        if v["key"] == key {
            return Some(v);
        }
        v["children"].as_array()?.iter().find_map(|c| find_key(c, key))
    }

    #[test]
    fn partial_selection_is_mixed_with_a_dash() {
        let mut rt = run(Toggles::default(), |_, _| line(true, true, true), (640.0, 200.0));
        let tree = rt.accessibility_json();
        let all = find_key(&tree, "check-all").expect("no check-all in the tree");
        assert!(all["states"].as_array().is_some_and(|s| s.iter().any(|x| x == "mixed")), "check-all is not mixed: {all}");
        // The kit draws its own check at 10 points and a given mark (the dash) at 14.
        let mark = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("mark")).expect("no mark");
        assert_eq!((mark.rect[2], mark.opacity), (14.0, 1.0), "the mark is not the dash: {:?}", mark.rect);
        click(&mut rt, "check-all");
        assert_eq!(rt.state.all, 1, "toggling a partial selection does nothing");
    }

    #[test]
    fn disabled_select_all_does_nothing() {
        let mut rt = run(Toggles::default(), |_, _| line(false, false, false), (640.0, 200.0));
        click(&mut rt, "check-all");
        assert_eq!(rt.state.all, 0);
    }

    #[test]
    fn pressing_the_count_clears() {
        let mut rt = run(Toggles::default(), |_, _| line(true, true, true), (640.0, 200.0));
        let words = rt.elements().into_iter().find(|e| e.text.as_deref() == Some("3 selected")).expect("no count");
        let (x, y) = (words.rect[0] + words.rect[2] * 0.5, words.rect[1] + words.rect[3] * 0.5);
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        assert_eq!(rt.state.cleared, 1, "pressing the words did not clear");
        let clear = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("bulk-clear")).expect("no bulk-clear");
        assert_eq!((clear.role, clear.label.as_deref(), clear.help.as_deref(), clear.focusable), ("button", Some("Clear selection"), Some("Clear selection (Esc)"), true));
    }
}
