//! CSS 2.1 display box generation before used values and line layout.
//!
//! This is a separate, bounded path; the shipping reader remains the fallback.
//! It preserves DOM order and text. Anonymous block wrappers follow CSS 2.1
//! §9.2.1.1 for mixed block and inline children. It does not claim to layout
//! those boxes, and calls out block-in-inline splitting as unsupported.

use crate::computed_style::{Computed, Display, Length};
use crate::style_tree::StyleTree;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoxKind {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Text,
    AnonymousBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CssBox {
    pub kind: BoxKind,
    /// Index into the styled arena; anonymous boxes have no DOM source.
    pub source: Option<usize>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub style: Computed,
    /// Original DOM text. White-space processing belongs to line layout.
    pub text: Option<String>,
}

#[derive(Default)]
pub struct BoxTree {
    pub boxes: Vec<CssBox>,
    pub roots: Vec<usize>,
    pub truncated: bool,
    pub unsupported: bool,
}

/// CSS 2.2 §10.3.3, restricted to zero margin/padding/border, LTR,
/// non-replaced block boxes in normal flow. These are content widths only.
/// A future full algorithm must include box edges and direction rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsedBlockWidth {
    pub content: u32,
    pub margin_left: i64,
    pub margin_right: i64,
}

/// Resolve a block's width from the containing block's content width. The
/// viewport supplies the initial containing block width; inline boxes have
/// different rules and must not be passed here. With zero (not auto) margins,
/// unused or negative space is assigned to the right margin in LTR.
#[must_use]
pub fn used_block_width(width: Length, containing: u32) -> UsedBlockWidth {
    let content = match width {
        Length::Auto => containing,
        Length::Px(px) => px,
        Length::Percent(hundredths) => {
            u32::try_from(u64::from(containing) * u64::from(hundredths) / 10_000)
                .unwrap_or(u32::MAX)
        }
    };
    UsedBlockWidth {
        content,
        margin_left: 0,
        margin_right: i64::from(containing) - i64::from(content),
    }
}

/// A definite specified height only; `auto` needs child layout, and a
/// percentage needs a definite containing-block content height. This is not
/// the used height of content that could overflow the specified height.
#[must_use]
pub fn specified_block_height(height: Length, containing: Option<u32>) -> Option<u32> {
    match height {
        Length::Auto => None,
        Length::Px(px) => Some(px),
        Length::Percent(hundredths) => containing.map(|base| {
            u32::try_from(u64::from(base) * u64::from(hundredths) / 10_000).unwrap_or(u32::MAX)
        }),
    }
}

/// Top-down diagnostic of definite block heights. Missing heights remain
/// unknown for descendant percentage heights. No y position is inferred.
pub struct HeightPass {
    pub heights: Vec<Option<u32>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl HeightPass {
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_height: u32) -> Self {
        let mut result = Self {
            heights: vec![None; tree.boxes.len()],
            unsupported: tree.unsupported,
            truncated: tree.truncated,
        };
        let mut stack: Vec<_> = tree
            .roots
            .iter()
            .rev()
            .map(|&root| (root, Some(viewport_height)))
            .collect();
        let mut visited = 0_usize;
        while let Some((index, containing)) = stack.pop() {
            visited += 1;
            if visited > tree.boxes.len() {
                result.truncated = true;
                break;
            }
            let node = &tree.boxes[index];
            let height = match node.kind {
                BoxKind::Block | BoxKind::ListItem => {
                    specified_block_height(node.style.height, containing)
                }
                BoxKind::AnonymousBlock => None,
                BoxKind::InlineBlock | BoxKind::Inline | BoxKind::Text => {
                    if node.kind == BoxKind::InlineBlock {
                        result.unsupported = true;
                    }
                    None
                }
            };
            result.heights[index] = height;
            for &child in node.children.iter().rev() {
                stack.push((child, height));
            }
        }
        result
    }
}

/// A width-only pass. `None` means this box is not a normal-flow block;
/// this is not a paintable rectangle (height, y position and line widths are
/// still missing). The reader remains the only user-facing renderer.
pub struct WidthPass {
    pub widths: Vec<Option<UsedBlockWidth>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl WidthPass {
    /// Compute content widths top-down using each block container's content
    /// width. Inline boxes pass through the nearest block container; inline-
    /// block needs shrink-to-fit and is explicitly unsupported here.
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_width: u32) -> Self {
        let mut result = Self {
            widths: vec![None; tree.boxes.len()],
            unsupported: tree.unsupported,
            truncated: tree.truncated,
        };
        let mut stack: Vec<_> = tree
            .roots
            .iter()
            .rev()
            .map(|&root| (root, viewport_width))
            .collect();
        let mut visited = 0_usize;
        while let Some((index, containing)) = stack.pop() {
            visited += 1;
            if visited > tree.boxes.len() {
                result.truncated = true;
                break;
            }
            let node = &tree.boxes[index];
            let content = match node.kind {
                BoxKind::Block | BoxKind::ListItem | BoxKind::AnonymousBlock => {
                    // Anonymous block boxes fill their containing block.
                    let specified = if node.kind == BoxKind::AnonymousBlock {
                        Length::Auto
                    } else {
                        node.style.width
                    };
                    let width = used_block_width(specified, containing);
                    result.widths[index] = Some(width);
                    width.content
                }
                BoxKind::InlineBlock => {
                    result.unsupported = true;
                    containing // preserve traversal, not a guessed used width
                }
                BoxKind::Inline | BoxKind::Text => containing,
            };
            for &child in node.children.iter().rev() {
                stack.push((child, content));
            }
        }
        result
    }
}

impl BoxTree {
    /// Convert a bounded styled DOM into boxes without painting or flattening
    /// mixed text. At most two boxes per styled node are allocated.
    #[must_use]
    pub fn from_style(styled: &StyleTree) -> Self {
        let mut tree = Self {
            truncated: styled.truncated,
            unsupported: styled.unsupported,
            ..Self::default()
        };
        let limit = styled.nodes.len().saturating_mul(2);
        let mut stack: Vec<_> = styled
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(index, _)| (index, None))
            .rev()
            .collect();
        while let Some((source, parent)) = stack.pop() {
            let node = &styled.nodes[source];
            let kind = if node.text.is_some() {
                Some(BoxKind::Text)
            } else {
                match node.style.display {
                    Display::None => None,
                    Display::Block => Some(BoxKind::Block),
                    Display::Inline => Some(BoxKind::Inline),
                    Display::InlineBlock => Some(BoxKind::InlineBlock),
                    Display::ListItem => Some(BoxKind::ListItem),
                }
            };
            let Some(kind) = kind else {
                continue; // display:none removes the whole subtree.
            };
            if tree.boxes.len() >= limit {
                tree.truncated = true;
                break;
            }
            let index = tree.boxes.len();
            tree.boxes.push(CssBox {
                kind,
                source: Some(source),
                parent,
                children: Vec::new(),
                style: node.style,
                text: node.text.clone(),
            });
            if let Some(parent) = parent {
                tree.boxes[parent].children.push(index);
            } else {
                tree.roots.push(index);
            }
            for &child in node.children.iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        // Iterate only the original boxes. Anonymous wrappers append later and
        // must not themselves be normalized again.
        let originals = tree.boxes.len();
        for parent in (0..originals).rev() {
            let kind = tree.boxes[parent].kind;
            let children = &tree.boxes[parent].children;
            let has_block = children
                .iter()
                .any(|&child| matches!(tree.boxes[child].kind, BoxKind::Block | BoxKind::ListItem));
            if kind == BoxKind::Inline && has_block {
                // CSS 2.1 §9.2.1.1 requires splitting inline boxes. Do not
                // mislabel this partial tree as a valid used box tree.
                tree.unsupported = true;
            }
            if !matches!(kind, BoxKind::Block | BoxKind::ListItem) || !has_block {
                continue;
            }
            let old = std::mem::take(&mut tree.boxes[parent].children);
            let mut run = Vec::new();
            for child in old {
                if matches!(tree.boxes[child].kind, BoxKind::Block | BoxKind::ListItem) {
                    tree.wrap_inline_run(parent, &mut run, limit);
                    tree.boxes[parent].children.push(child);
                } else {
                    run.push(child);
                }
            }
            tree.wrap_inline_run(parent, &mut run, limit);
        }
        tree
    }

    fn wrap_inline_run(&mut self, parent: usize, run: &mut Vec<usize>, limit: usize) {
        if run.is_empty() {
            return;
        }
        if self.boxes.len() >= limit {
            self.truncated = true;
            // Keep surviving children reachable even when a wrapper won't fit.
            self.boxes[parent].children.append(run);
            return;
        }
        let index = self.boxes.len();
        let style = self.boxes[parent].style;
        for &child in run.iter() {
            self.boxes[child].parent = Some(index);
        }
        self.boxes.push(CssBox {
            kind: BoxKind::AnonymousBlock,
            source: None,
            parent: Some(parent),
            children: std::mem::take(run),
            style,
            text: None,
        });
        self.boxes[parent].children.push(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_style_tree, Limits};

    fn boxes(html: &str) -> BoxTree {
        BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
    }

    #[test]
    fn definite_height_does_not_guess_auto_or_indefinite_percent() {
        assert_eq!(specified_block_height(Length::Px(80), None), Some(80));
        assert_eq!(specified_block_height(Length::Auto, Some(300)), None);
        assert_eq!(specified_block_height(Length::Percent(2500), None), None);
        assert_eq!(
            specified_block_height(Length::Percent(2500), Some(300)),
            Some(75)
        );
        assert_eq!(
            specified_block_height(Length::Percent(10_000), Some(u32::MAX)),
            Some(u32::MAX)
        );
        let tree = boxes("<html style='height:400px'><body style='height:50%'><main style='height:25%'>a</main><aside><div style='height:20%'>b</div></aside></body></html>");
        let pass = HeightPass::from_boxes(&tree, 600);
        let styled = parse_style_tree(b"<html style='height:400px'><body style='height:50%'><main style='height:25%'>a</main><aside><div style='height:20%'>b</div></aside></body></html>", &[], &Limits::DEFAULT);
        let height = |tag: &str| {
            tree.boxes
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    b.source
                        .is_some_and(|source| styled.nodes[source].tag == tag)
                })
                .and_then(|(index, _)| pass.heights[index])
        };
        assert_eq!(height("html"), Some(400));
        assert_eq!(height("body"), Some(200));
        assert_eq!(height("main"), Some(50));
        assert_eq!(height("aside"), None);
        assert_eq!(height("div"), None);
        assert!(!pass.truncated);
    }

    #[test]
    fn css_2_2_block_width_with_zero_edges() {
        assert_eq!(
            used_block_width(Length::Auto, 300),
            UsedBlockWidth {
                content: 300,
                margin_left: 0,
                margin_right: 0
            }
        );
        assert_eq!(
            used_block_width(Length::Px(200), 301),
            UsedBlockWidth {
                content: 200,
                margin_left: 0,
                margin_right: 101
            }
        );
        assert_eq!(
            used_block_width(Length::Percent(2500), 400),
            UsedBlockWidth {
                content: 100,
                margin_left: 0,
                margin_right: 300
            }
        );
        assert_eq!(
            used_block_width(Length::Px(360), 300),
            UsedBlockWidth {
                content: 360,
                margin_left: 0,
                margin_right: -60
            }
        );
        assert_eq!(
            used_block_width(Length::Percent(10_000), u32::MAX).content,
            u32::MAX
        );
    }

    #[test]
    fn widths_follow_nearest_block_content_width() {
        let tree = boxes("<style>body{width:50%}section{width:25%}</style><body><section><div>Text</div></section></body>");
        let used = WidthPass::from_boxes(&tree, 400);
        let blocks: Vec<_> = tree
            .boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == BoxKind::Block)
            .map(|(i, _)| used.widths[i].unwrap().content)
            .collect();
        assert_eq!(blocks, [400, 200, 50, 50]); // html, body, section, div
        assert!(!used.unsupported);
        assert!(!used.truncated);
        assert_eq!(used.widths.len(), tree.boxes.len());
    }

    #[test]
    fn inline_block_width_is_not_guessed() {
        let tree = boxes("<div><span style='display:inline-block;width:80px'>A</span></div>");
        let used = WidthPass::from_boxes(&tree, 300);
        let index = tree
            .boxes
            .iter()
            .position(|b| b.kind == BoxKind::InlineBlock)
            .unwrap();
        assert_eq!(used.widths[index], None);
        assert!(used.unsupported);
    }

    #[test]
    fn mixed_children_get_anonymous_blocks_in_dom_order() {
        let tree = boxes("<div>A <em>B</em><p>C</p> D <i>E</i></div>");
        let div = tree
            .boxes
            .iter()
            .position(|b| b.source.is_some_and(|s| s == 3))
            .unwrap();
        let kinds: Vec<_> = tree.boxes[div]
            .children
            .iter()
            .map(|&i| tree.boxes[i].kind)
            .collect();
        assert_eq!(
            kinds,
            [
                BoxKind::AnonymousBlock,
                BoxKind::Block,
                BoxKind::AnonymousBlock
            ]
        );
        let first = tree.boxes[div].children[0];
        assert_eq!(tree.boxes[first].children.len(), 2);
        assert!(tree.boxes[first]
            .children
            .iter()
            .all(|&i| tree.boxes[i].parent == Some(first)));
        assert!(!tree.truncated);
    }

    #[test]
    fn all_inline_children_stay_in_inline_formatting_context() {
        let tree = boxes("<div>One <em>two</em> three</div>");
        assert!(!tree.boxes.iter().any(|b| b.kind == BoxKind::AnonymousBlock));
        assert!(tree.boxes.iter().any(|b| b.text.as_deref() == Some("One ")));
    }

    #[test]
    fn hidden_subtree_has_no_boxes_or_text() {
        let tree =
            boxes("<div>Shown<span style='display:none'>Secret<b>Nested</b></span> End</div>");
        assert!(!tree
            .boxes
            .iter()
            .any(|b| b.text.as_deref() == Some("Secret") || b.text.as_deref() == Some("Nested")));
        assert!(tree.boxes.iter().any(|b| b.text.as_deref() == Some(" End")));
    }

    #[test]
    fn block_inside_inline_is_explicitly_unsupported() {
        let tree = boxes("<span>before<div>block</div>after</span>");
        assert!(tree.unsupported);
    }
}
