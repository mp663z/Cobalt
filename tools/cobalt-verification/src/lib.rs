//! Host-only tests against production browser code. No copied implementations.
#[cfg(test)]
mod tests {
    use kobo_web_layout::{paginate_pieces, Piece};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        #[test]
        fn arbitrary_short_frames_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            let _ = kobo_protocol::decode(&bytes);
        }
        #[test]
        fn whole_text_keeps_unicode_and_blank_lines(text in "[a-zé界\\n]{1,24}") {
            let layout = paginate_pieces(vec![Piece::Preformatted(text.clone())], &[], |_| true);
            prop_assert_eq!(layout.pages.len(), 1);
            match &layout.pages[0][0] {
                Piece::Preformatted(actual) => prop_assert_eq!(actual, &text),
                _ => prop_assert!(false, "piece changed kind"),
            }
        }
        #[test]
        fn bounded_whole_pieces_keep_order(items in prop::collection::vec("[a-z]{1,8}", 0..12)) {
            let pieces = items.iter().cloned().map(Piece::Preformatted).collect();
            let layout = paginate_pieces(pieces, &[], |page| page.len() <= 1);
            let actual: Vec<_> = layout.pages.iter().flatten().map(|piece| match piece {
                Piece::Preformatted(text) => text.clone(),
                _ => panic!("piece changed kind"),
            }).collect();
            prop_assert_eq!(actual, items);
            prop_assert!(layout.pages.iter().all(|page| page.len() <= 1));
        }
    }

    #[test]
    fn newline_split_loss_is_recorded_not_called_preserved() {
        let layout = paginate_pieces(vec![Piece::Preformatted("a\n\nz".into())], &[], |page| {
            page.iter()
                .map(|piece| match piece {
                    Piece::Preformatted(text) => text.len(),
                    _ => 0,
                })
                .sum::<usize>()
                <= 3
        });
        let text: String = layout
            .pages
            .iter()
            .flatten()
            .map(|piece| match piece {
                Piece::Preformatted(text) => text.as_str(),
                _ => "",
            })
            .collect();
        // Current production split trims boundary newlines. This test records
        // the defect and must be replaced with equality once semantics are fixed.
        assert_ne!(text, "a\n\nz");
    }
}
