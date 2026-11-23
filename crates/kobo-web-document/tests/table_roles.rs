//! CSS table roles are retained as roles, not disguised as block/flex boxes.
//! Grid paint is deliberately refused until its geometry path is integrated.
use kobo_web_document::{
    box_tree::{BoxKind, BoxTree},
    computed_style::Display,
    Limits, SupplyBudget,
};

fn tree(html: &str) -> (kobo_web_document::style_tree::StyleTree, BoxTree) {
    let style = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    let boxes = BoxTree::from_style(&style);
    (style, boxes)
}

#[test]
fn author_table_roles_survive_box_generation() {
    let (style, boxes) = tree("<!doctype html><style>.grid{display:table}.group{display:table-row-group}.row{display:table-row}.cell{display:table-cell}</style><div class=grid><div class=group><div class=row><div class=cell>A</div><div class=cell>B</div></div></div></div>");
    for (display, kind, expected) in [
        (Display::Table, BoxKind::Table, 1),
        (Display::TableRowGroup, BoxKind::TableRowGroup, 1),
        (Display::TableRow, BoxKind::TableRow, 1),
        (Display::TableCell, BoxKind::TableCell, 2),
    ] {
        assert_eq!(
            style
                .nodes
                .iter()
                .filter(|node| node.style.display == display && node.text.is_none())
                .count(),
            expected
        );
        assert_eq!(
            boxes.boxes.iter().filter(|node| node.kind == kind).count(),
            expected
        );
    }
}

#[test]
fn original_html_spans_are_retained_without_rewriting() {
    let (style, _) =
        tree("<!doctype html><table><tr><td colspan=3 rowspan=2>A</td><td>B</td></tr></table>");
    let cells: Vec<_> = style.nodes.iter().filter(|node| node.tag == "td").collect();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].table_span, (3, 2));
    assert_eq!(cells[1].table_span, (1, 1));
}

#[test]
fn hidden_table_subtree_has_no_boxes() {
    let (_, boxes) = tree("<!doctype html><div style='display:none'><div style='display:table'><div style='display:table-row'><div style='display:table-cell'>hidden</div></div></div></div>");
    assert!(boxes.boxes.iter().all(|node| node.kind != BoxKind::Table));
    assert!(boxes
        .boxes
        .iter()
        .all(|node| node.text.as_deref() != Some("hidden")));
}
