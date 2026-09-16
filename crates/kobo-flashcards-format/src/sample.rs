//! The built-in sample collection: a few original cards that ship inside the
//! app, so a first run can offer something to review before any import.
//!
//! The same builder writes the committed simulator fixture, so what a first
//! run offers is exactly what routes and screenshots exercise. Regenerate
//! the fixture with the crate's `demo_bundle` example.

use crate::{
    encode, BundleManifest, Card, Deck, DeckConfiguration, DeckQueue, FormatError, Note, NoteType,
    ReviewQueue, Source, CONVERTER_REVISION,
};
use std::collections::BTreeMap;

/// Front and back text of the sample cards. Original, Cobalt-owned content.
const SAMPLE_CARDS: [(&str, &str); 3] = [
    ("What does a compass point toward?", "Magnetic north."),
    ("What is water turning into vapour called?", "Evaporation."),
    ("How many sides does a hexagon have?", "Six."),
];

fn sample_source() -> Source {
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
        note_count: SAMPLE_CARDS.len(),
        card_count: SAMPLE_CARDS.len(),
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

fn sample_manifest() -> BundleManifest {
    let mut manifest = BundleManifest::empty(sample_source());
    manifest.deck_configurations.push(DeckConfiguration {
        id: 1,
        name: "Default".to_owned(),
        original_json: "{}".to_owned(),
    });
    manifest.decks.push(Deck {
        id: 1,
        name: "Nature Notes".to_owned(),
        configuration_id: Some(1),
        original_json: "{}".to_owned(),
    });
    manifest.notetypes.push(NoteType {
        id: 1,
        name: "Basic".to_owned(),
        original_json: "{}".to_owned(),
    });
    let mut card_ids = Vec::new();
    for (offset, (front, back)) in SAMPLE_CARDS.iter().enumerate() {
        let id = i64::try_from(offset + 1).unwrap_or(1);
        card_ids.push(id);
        manifest.notes.push(Note {
            id,
            guid: format!("demo-{id}"),
            notetype_id: 1,
            modified: 1,
            user_sequence: 0,
            tags: Vec::new(),
            fields: vec![(*front).to_owned()],
            sort_field: (*front).to_owned(),
            checksum: 1,
            flags: 0,
            data: String::new(),
        });
        manifest.cards.push(Card {
            id,
            note_id: id,
            deck_id: 1,
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
            front: (*front).to_owned(),
            back: (*back).to_owned(),
            front_spans: Vec::new(),
            back_spans: Vec::new(),
            tags: Vec::new(),
            question_media_names: Vec::new(),
            answer_media_names: Vec::new(),
            media_names: Vec::new(),
            attachments: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let count = card_ids.len();
    manifest.review_queue = ReviewQueue {
        card_ids: card_ids.clone(),
        new_count: count,
        learning_count: 0,
        review_count: 0,
        decks: vec![DeckQueue {
            source_index: 0,
            root_deck_id: 1,
            card_ids,
            new_count: count,
            learning_count: 0,
            review_count: 0,
        }],
    };
    manifest
}

/// Encodes the sample collection exactly as a staged import would produce it.
///
/// # Errors
///
/// Returns an error when the sample cannot be represented; that is a build
/// defect, not reader state, so callers should say the sample is unavailable.
pub fn sample_bundle() -> Result<Vec<u8>, FormatError> {
    encode(sample_manifest(), BTreeMap::new())
}
