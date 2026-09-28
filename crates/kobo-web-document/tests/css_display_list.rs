#[test]
fn complete_paint_lists_combine_in_order_without_partial_budget_failure() {
    let mut backgrounds = DisplayList::default();
    backgrounds
        .fill(rect(0, 0, 3, 2), Rgb(255, 0, 0), Source::default())
        .unwrap();
    let mut text = DisplayList::default();
    text.glyph_run(
        rect(1, 0, 1, 1),
        vec![255],
        Rgb(0, 0, 0),
        Source {
            node: Some(4),
            action: None,
        },
    )
    .unwrap();
    backgrounds.append_list(text).unwrap();
    assert_eq!(backgrounds.commands().len(), 2);
    assert_eq!(backgrounds.glyph_bytes(), 1);
    let pixels = backgrounds.rasterize(3, 2, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[4..8], &[0, 0, 0, 255]);
    assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);

    let mut full = DisplayList::default();
    for _ in 0..MAX_COMMANDS {
        full.fill(rect(0, 0, 0, 0), Rgb(0, 0, 0), Source::default())
            .unwrap();
    }
    let mut extra = DisplayList::default();
    extra
        .fill(rect(0, 0, 1, 1), Rgb(0, 0, 0), Source::default())
        .unwrap();
    assert_eq!(full.append_list(extra), Err(Error::TooManyCommands));
    assert_eq!(full.commands().len(), MAX_COMMANDS);
    assert_eq!(full.glyph_bytes(), 0);

    let mut a = DisplayList::default();
    a.glyph_run(
        rect(0, 0, u32::try_from(MAX_GLYPH_BYTES).unwrap(), 1),
        vec![0; MAX_GLYPH_BYTES],
        Rgb(0, 0, 0),
        Source::default(),
    )
    .unwrap();
    let mut b = DisplayList::default();
    b.glyph_run(rect(0, 0, 1, 1), vec![255], Rgb(0, 0, 0), Source::default())
        .unwrap();
    assert_eq!(a.append_list(b), Err(Error::GlyphBudget));
    assert_eq!(a.commands().len(), 1);
    assert_eq!(a.glyph_bytes(), MAX_GLYPH_BYTES);

    let mut open = DisplayList::default();
    open.push_clip(rect(0, 0, 1, 1)).unwrap();
    assert_eq!(backgrounds.append_list(open), Err(Error::UnbalancedClip));
    assert_eq!(backgrounds.commands().len(), 2);
}

#[path = "../src/display_list.rs"]
mod display_list;
use display_list::{
    Command, DisplayList, Error, Rect, Rgb, Source, MAX_CLIP_DEPTH, MAX_COMMANDS, MAX_GLYPH_BYTES,
    MAX_PIXELS,
};

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}
fn rgba(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255]
}

#[test]
fn overlap_nested_clip_and_mask_exact_pixels() {
    let mut list = DisplayList::default();
    let source = Source {
        node: Some(9),
        action: Some(3),
    };
    list.fill(rect(0, 0, 4, 3), Rgb(255, 0, 0), source).unwrap();
    list.push_clip(rect(1, 0, 2, 3)).unwrap();
    list.fill(rect(0, 1, 4, 2), Rgb(0, 0, 255), source).unwrap();
    list.push_clip(rect(2, 1, 2, 1)).unwrap();
    list.glyph_run(rect(1, 1, 2, 1), vec![255, 128], Rgb(0, 255, 0), source)
        .unwrap();
    list.pop_clip().unwrap();
    list.pop_clip().unwrap();
    assert_eq!(list.commands().len(), 7);
    assert_eq!(list.glyph_bytes(), 2);
    assert!(matches!(&list.commands()[4], Command::GlyphRun { source: s, .. } if *s == source));
    let actual = list.rasterize(4, 3, Rgb(255, 255, 255)).unwrap();
    let expected: Vec<u8> = [
        [255, 0, 0],
        [255, 0, 0],
        [255, 0, 0],
        [255, 0, 0],
        [255, 0, 0],
        [0, 0, 255],
        [0, 128, 127],
        [255, 0, 0],
        [255, 0, 0],
        [0, 0, 255],
        [0, 0, 255],
        [255, 0, 0],
    ]
    .into_iter()
    .flat_map(rgba)
    .collect();
    assert_eq!(actual, expected);
}

#[test]
fn negative_oversize_and_empty_intersections_are_safe_and_exact() {
    let mut list = DisplayList::default();
    list.fill(rect(-2, -1, 4, 3), Rgb(1, 2, 3), Source::default())
        .unwrap();
    list.push_clip(rect(i32::MIN, 1, u32::MAX, u32::MAX))
        .unwrap();
    list.fill(
        rect(i32::MAX, i32::MAX, u32::MAX, u32::MAX),
        Rgb(8, 9, 10),
        Source::default(),
    )
    .unwrap();
    list.fill(
        rect(0, 1, u32::MAX, u32::MAX),
        Rgb(4, 5, 6),
        Source::default(),
    )
    .unwrap();
    list.pop_clip().unwrap();
    let expected: Vec<u8> = [
        [1, 2, 3],
        [1, 2, 3],
        [255, 255, 255],
        [4, 5, 6],
        [4, 5, 6],
        [4, 5, 6],
    ]
    .into_iter()
    .flat_map(rgba)
    .collect();
    assert_eq!(list.rasterize(3, 2, Rgb(255, 255, 255)).unwrap(), expected);
}

#[test]
fn hard_limits_and_failed_operations_do_not_change_state() {
    let mut list = DisplayList::default();
    assert_eq!(list.pop_clip(), Err(Error::UnbalancedClip));
    assert_eq!(
        list.glyph_run(rect(0, 0, 2, 1), vec![1], Rgb(0, 0, 0), Source::default()),
        Err(Error::InvalidCoverage)
    );
    assert_eq!(
        list.glyph_run(
            rect(0, 0, u32::try_from(MAX_GLYPH_BYTES + 1).unwrap(), 1),
            vec![0; MAX_GLYPH_BYTES + 1],
            Rgb(0, 0, 0),
            Source::default()
        ),
        Err(Error::GlyphBudget)
    );
    assert_eq!(list.glyph_bytes(), 0);
    assert!(list.commands().is_empty());
    for _ in 0..MAX_CLIP_DEPTH {
        list.push_clip(rect(0, 0, 1, 1)).unwrap();
    }
    assert_eq!(list.push_clip(rect(0, 0, 1, 1)), Err(Error::TooManyClips));
    assert_eq!(
        list.rasterize(1, 1, Rgb(0, 0, 0)),
        Err(Error::UnbalancedClip)
    );
    for _ in 0..MAX_CLIP_DEPTH {
        list.pop_clip().unwrap();
    }
    while list.commands().len() < MAX_COMMANDS {
        list.fill(rect(0, 0, 0, 0), Rgb(0, 0, 0), Source::default())
            .unwrap();
    }
    assert_eq!(
        list.fill(rect(0, 0, 1, 1), Rgb(0, 0, 0), Source::default()),
        Err(Error::TooManyCommands)
    );
    assert_eq!(list.commands().len(), MAX_COMMANDS);
    assert_eq!(
        list.rasterize(u32::try_from(MAX_PIXELS + 1).unwrap(), 1, Rgb(0, 0, 0)),
        Err(Error::InvalidSurface)
    );
    assert_eq!(
        list.rasterize(u32::MAX, u32::MAX, Rgb(0, 0, 0)),
        Err(Error::InvalidSurface)
    );
}
