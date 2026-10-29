use super::*;
use egui::{Event, Pos2, RawInput};
use std::collections::HashMap as StringMap;

/// Entries that paint exactly `paint` tall but guess `guess`.
struct Fixed {
    paint: Vec<f32>,
    guess: Vec<f32>,
}

impl Fixed {
    fn uniform(len: usize, height: f32) -> Self {
        Fixed {
            paint: vec![height; len],
            guess: vec![height; len],
        }
    }
}

impl Entries for Fixed {
    fn len(&self) -> usize {
        self.paint.len()
    }
    fn estimate(&mut self, _: &Ui, index: usize, _: f32) -> f32 {
        self.guess[index]
    }
    fn paint(&mut self, ui: &mut Ui, index: usize) {
        ui.allocate_exact_size(
            Vec2::new(ui.available_width(), self.paint[index]),
            Sense::hover(),
        );
    }
    fn empty(&mut self, ui: &mut Ui) {
        ui.label("empty");
    }
}

fn key(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

/// One frame, 1000x700: wide enough for a two-page spread.
fn frame(ctx: &Context, events: Vec<Event>, mut draw: impl FnMut(&mut Ui)) {
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0))),
            events,
            ..Default::default()
        },
        |ui| draw(ui),
    );
    output.textures_delta.clear();
}

#[test]
fn page_keys_turn_one_spread_and_home_and_end_reach_the_ends() {
    let ctx = Context::default();
    crate::theme::apply(&ctx);
    let mut book = Book::default();
    let mut entries = Fixed::uniform(60, 100.0);
    let mut press = |book: &mut Book, k: Key| {
        frame(&ctx, vec![key(k)], |ui| {
            book.show(ui, "Spellbook", &mut entries)
        });
        book.anchor
    };
    frame(&ctx, vec![], |ui| {
        book.show(ui, "Spellbook", &mut Fixed::uniform(60, 100.0))
    });
    assert_eq!(book.anchor, 0);

    let second = press(&mut book, Key::PageDown);
    assert!(second > 0, "PageDown did not turn");
    let third = press(&mut book, Key::PageDown);
    assert!(third > second);
    assert_eq!(press(&mut book, Key::PageUp), second);
    assert_eq!(press(&mut book, Key::Home), 0);
    assert_eq!(
        press(&mut book, Key::PageUp),
        0,
        "the first spread has no previous"
    );
    let last = press(&mut book, Key::End);
    assert!(last > third);
    assert_eq!(
        press(&mut book, Key::PageDown),
        last,
        "the last spread has no next"
    );
}

#[test]
fn page_keys_do_nothing_while_a_text_field_has_focus() {
    let ctx = Context::default();
    crate::theme::apply(&ctx);
    let mut book = Book::default();
    let mut entries = Fixed::uniform(60, 100.0);
    let mut text = String::from("typing");
    for events in [vec![], vec![], vec![key(Key::PageDown), key(Key::End)]] {
        frame(&ctx, events, |ui| {
            let field = ui.text_edit_singleline(&mut text);
            if !field.has_focus() {
                field.request_focus();
            }
            book.show(ui, "Spellbook", &mut entries);
        });
    }
    assert!(ctx.text_edit_focused());
    assert_eq!(book.anchor, 0);
}

#[test]
fn a_focused_page_turn_button_does_not_block_the_keys() {
    let ctx = Context::default();
    crate::theme::apply(&ctx);
    let mut book = Book::default();
    let mut entries = Fixed::uniform(60, 100.0);
    frame(&ctx, vec![], |ui| book.show(ui, "Spellbook", &mut entries));
    // Any focused widget makes `egui_wants_keyboard_input` true; the book
    // must still turn while it is a button, not a text field.
    ctx.memory_mut(|m| m.request_focus(Id::new("some-button")));
    frame(&ctx, vec![key(Key::PageDown)], |ui| {
        ui.add(egui::Button::new("x")).surrender_focus();
        book.show(ui, "Spellbook", &mut entries);
    });
    assert!(book.anchor > 0);
}

#[test]
fn painted_heights_replace_a_wrong_estimate() {
    let ctx = Context::default();
    crate::theme::apply(&ctx);
    let mut book = Book::default();
    let mut entries = Fixed {
        paint: vec![150.0; 12],
        guess: vec![10.0; 12],
    };
    for _ in 0..3 {
        frame(&ctx, vec![], |ui| book.show(ui, "Spellbook", &mut entries));
    }
    // The first spread's entries were painted, so they are paginated by
    // their real height from now on.
    assert_eq!(book.painted.get(&0), Some(&150.0));
}

#[test]
fn the_anchor_clamps_when_entries_are_deleted_under_it() {
    let ctx = Context::default();
    crate::theme::apply(&ctx);
    let mut book = Book {
        anchor: 40,
        ..Default::default()
    };
    frame(&ctx, vec![], |ui| {
        book.show(ui, "Spellbook", &mut Fixed::uniform(5, 100.0))
    });
    assert_eq!(book.anchor, 4);
    frame(&ctx, vec![], |ui| {
        book.show(ui, "Spellbook", &mut Fixed::uniform(0, 100.0))
    });
    assert_eq!(book.anchor, 0);
}

#[test]
fn the_turn_row_names_the_open_pages() {
    assert_eq!(pages_label(0, 2, 12), "Pages 1\u{2013}2 of 12");
    assert_eq!(pages_label(5, 2, 11), "Page 11 of 11");
    assert_eq!(pages_label(2, 1, 12), "Page 3 of 12");
    assert_eq!(pages_label(0, 2, 1), "Page 1 of 1");
}

#[derive(Default)]
struct MemoryStorage(StringMap<String, String>);

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

#[test]
fn the_view_defaults_to_list_and_persists_per_page() {
    let (spells, invocations) = (Id::new("spells-view"), Id::new("invocations-view"));
    let ctx = Context::default();
    assert_eq!(view(&ctx, spells), BookView::List);
    set_view(&ctx, spells, BookView::Book);
    assert_eq!(view(&ctx, spells), BookView::Book);
    assert_eq!(
        view(&ctx, invocations),
        BookView::List,
        "pages are independent"
    );

    let mut storage = MemoryStorage::default();
    ctx.memory(|memory| eframe::set_value(&mut storage, "egui", memory));
    let restored = Context::default();
    let memory = eframe::get_value(&storage, "egui").expect("serialized egui memory");
    restored.memory_mut(|current| *current = memory);
    assert_eq!(view(&restored, spells), BookView::Book);
    assert_eq!(view(&restored, invocations), BookView::List);

    restored.data_mut(|d| d.insert_persisted(invocations, "future-view".to_owned()));
    assert_eq!(view(&restored, invocations), BookView::List);
}
