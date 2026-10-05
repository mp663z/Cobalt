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
        fn preformatted_line_cuts_keep_blank_lines(blank_count in 1usize..8) {
            let text = format!("a{}z", "\n".repeat(blank_count + 1));
            let layout = paginate_pieces(vec![Piece::Preformatted(text.clone())], &[], |page| {
                page.iter().map(|piece| match piece {
                    Piece::Preformatted(text) => text.len(), _ => 0,
                }).sum::<usize>() <= 3
            });
            let chunks: Vec<_> = layout.pages.iter().flatten().map(|piece| match piece {
                Piece::Preformatted(text) => text.as_str(),
                _ => panic!("piece changed kind"),
            }).collect();
            prop_assert_eq!(chunks.join("\n"), text);
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
    fn preformatted_line_split_preserves_blank_lines() {
        let layout = paginate_pieces(vec![Piece::Preformatted("a\n\nz".into())], &[], |page| {
            page.iter()
                .map(|piece| match piece {
                    Piece::Preformatted(text) => text.len(),
                    _ => 0,
                })
                .sum::<usize>()
                <= 3
        });
        let chunks: Vec<_> = layout
            .pages
            .iter()
            .flatten()
            .map(|piece| match piece {
                Piece::Preformatted(text) => text.as_str(),
                _ => panic!("piece changed kind"),
            })
            .collect();
        // Line cuts remove exactly one separator from the head.
        assert_eq!(chunks.join("\n"), "a\n\nz");
    }
}
