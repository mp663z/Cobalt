//! Writes the rich study bundle used to prove Japanese, media and long cards
//! on the simulator. Built with the same `encode` a staged import produces,
//! so the reader sees exactly what a host import of such cards yields.
//! Regenerate the committed fixture with:
//!
//! ```sh
//! cargo run -p kobo-flashcards-format --example rich_bundle -- \
//!     scripts/fixtures/flashcards/rich.cobfc
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use kobo_flashcards_format::{
    encode, Attachment, AttachmentKind, Card, Deck, DeckConfiguration, DeckQueue, Note, NoteType,
    ReviewQueue, Source, CONVERTER_REVISION,
};

fn source(note_count: usize) -> Source {
    Source {
        package_kind: "demo".to_owned(),
        collection_member: "generated".to_owned(),
        collection_schema: 0,
        normalized_schema: 0,
        collection_id: 1,
        collection_created: 0,
        collection_modified: 1,
        schema_modified: 1,
        dirty: 0,
        user_sequence: 0,
        last_sync: 0,
        note_count,
        card_count: note_count,
        converter_revision: CONVERTER_REVISION.to_owned(),
        original_config_json: "{}".to_owned(),
        original_models_json: "{}".to_owned(),
        original_decks_json: "{}".to_owned(),
        original_deck_configurations_json: "{}".to_owned(),
        original_tags_json: "{}".to_owned(),
        normalized_config: Vec::new(),
        normalized_tags: Vec::new(),
    }
}

fn card(id: i64, deck_id: i64, front: &str, back: &str) -> Card {
    Card {
        id,
        note_id: id,
        deck_id,
        ordinal: 0,
        user_sequence: 0,
        queue: 0,
        card_type: 0,
        due: id,
        interval: 0,
        ease_factor: 0,
        repetitions: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: 0,
        flags: 0,
        data: String::new(),
        modified: 1,
        template_name: "Card".to_owned(),
        front: front.to_owned(),
        back: back.to_owned(),
        front_spans: Vec::new(),
        back_spans: Vec::new(),
        tags: Vec::new(),
        question_media_names: Vec::new(),
        answer_media_names: Vec::new(),
        media_names: Vec::new(),
        attachments: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn note(id: i64, front: &str) -> Note {
    Note {
        id,
        guid: format!("rich-{id}"),
        notetype_id: 1,
        modified: 1,
        user_sequence: 0,
        tags: Vec::new(),
        fields: vec![front.to_owned()],
        sort_field: front.to_owned(),
        checksum: id,
        flags: 0,
        data: String::new(),
    }
}

fn rich_manifest() -> kobo_flashcards_format::BundleManifest {
    let long_text = "読み書きの練習には長い文章も必要です。このカードは何ページにも分かれ、操作はいつも画面の下に残ります。".repeat(40);
    let cards: [(i64, i64, String, String); 4] = [
        (
            1,
            1,
            "この言葉を読みます。".to_owned(),
            "答えは日本語です。".to_owned(),
        ),
        (
            2,
            1,
            "東京は日本の首都ですか？".to_owned(),
            "はい、東京が首都です。".to_owned(),
        ),
        (3, 2, long_text.clone(), long_text),
        (
            4,
            3,
            "What does this picture show?".to_owned(),
            "A sun.".to_owned(),
        ),
    ];
    let mut manifest = kobo_flashcards_format::BundleManifest::empty(source(cards.len()));
    manifest.deck_configurations.push(DeckConfiguration {
        id: 1,
        name: "Default".to_owned(),
        original_json: "{}".to_owned(),
    });
    manifest.notetypes.push(NoteType {
        id: 1,
        name: "Basic".to_owned(),
        original_json: "{}".to_owned(),
    });
    for (id, name) in [(1, "Japanese"), (2, "Long reads"), (3, "Pictures")] {
        manifest.decks.push(Deck {
            id,
            name: name.to_owned(),
            configuration_id: Some(1),
            original_json: "{}".to_owned(),
        });
    }
    let mut card_ids = Vec::new();
    for (id, deck_id, front, back) in cards {
        card_ids.push(id);
        manifest.notes.push(note(id, &front));
        let mut card = card(id, deck_id, &front, &back);
        if deck_id == 3 {
            card.question_media_names = vec!["sun.png".to_owned()];
            card.media_names = vec!["sun.png".to_owned()];
            card.attachments = vec![Attachment {
                name: "sun.png".to_owned(),
                rendered_name: None,
                mime: "image/png".to_owned(),
                kind: AttachmentKind::Image,
            }];
        }
        manifest.cards.push(card);
    }
    manifest.review_queue = ReviewQueue {
        card_ids: card_ids.clone(),
        new_count: card_ids.len(),
        learning_count: 0,
        review_count: 0,
        decks: vec![
            DeckQueue {
                source_index: 0,
                root_deck_id: 1,
                card_ids: vec![1, 2],
                new_count: 2,
                learning_count: 0,
                review_count: 0,
            },
            DeckQueue {
                source_index: 0,
                root_deck_id: 2,
                card_ids: vec![3],
                new_count: 1,
                learning_count: 0,
                review_count: 0,
            },
            DeckQueue {
                source_index: 0,
                root_deck_id: 3,
                card_ids: vec![4],
                new_count: 1,
                learning_count: 0,
                review_count: 0,
            },
        ],
    };

    manifest
}

fn main() -> ExitCode {
    let Some(output) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: rich_bundle OUTPUT.cobfc");
        return ExitCode::FAILURE;
    };
    if output.exists() {
        eprintln!("refusing to overwrite {}", output.display());
        return ExitCode::FAILURE;
    }
    let manifest = rich_manifest();
    let mut media = BTreeMap::new();
    media.insert(
        "sun.png".to_owned(),
        include_bytes!("../../../scripts/fixtures/flashcards/media/sun.png").to_vec(),
    );
    match encode(manifest, media) {
        Ok(bytes) => match std::fs::write(&output, bytes) {
            Ok(()) => {
                println!("wrote rich bundle to {}", output.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("could not write {}: {error}", output.display());
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("could not encode rich bundle: {error}");
            ExitCode::FAILURE
        }
    }
}
