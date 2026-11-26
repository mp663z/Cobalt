//! Bounded exhaustive comparison to a separate browser-history model.
//! This is not proptest or model-checker verification.
use kobo_browser_core::{Go, History, MAX_HISTORY};
use kobo_web_document::Url;

#[derive(Clone, Debug, Default)]
struct Model {
    entries: Vec<(usize, usize, Option<usize>)>,
    cursor: usize,
}

#[derive(Debug, PartialEq)]
enum Expected {
    Stay,
    Load(usize),
    Fragment(usize),
    Page(usize),
}

// IDs 0 and 1 are different fragments of one document. ID 2 is another document.
fn same_document(a: usize, b: usize) -> bool {
    (a < 2) == (b < 2)
}

fn apply_model(m: &mut Model, action: usize) -> Expected {
    match action {
        0..=2 => {
            if m.entries.get(m.cursor).map(|e| e.0) == Some(action) {
                return Expected::Stay;
            }
            let result = if m
                .entries
                .get(m.cursor)
                .is_some_and(|e| same_document(e.0, action))
            {
                Expected::Fragment(action)
            } else {
                Expected::Load(action)
            };
            if !m.entries.is_empty() {
                m.entries.truncate(m.cursor + 1);
            }
            m.entries.push((action, 0, None));
            if m.entries.len() > MAX_HISTORY {
                m.entries.remove(0);
            }
            m.cursor = m.entries.len() - 1;
            result
        }
        3 | 4 => {
            let next = if action == 3 {
                m.cursor.checked_sub(1)
            } else if m.cursor + 1 < m.entries.len() {
                Some(m.cursor + 1)
            } else {
                None
            };
            let Some(next) = next else {
                return Expected::Stay;
            };
            let previous = m.entries[m.cursor].0;
            m.cursor = next;
            let entry = m.entries[next];
            if same_document(previous, entry.0) {
                Expected::Page(entry.1)
            } else {
                Expected::Load(entry.0)
            }
        }
        5 => {
            if let Some(e) = m.entries.get_mut(m.cursor) {
                e.1 = 7;
                e.2 = Some(19);
            }
            Expected::Stay
        }
        6 => {
            // Restore an earlier snapshot position, including the empty case.
            if !m.entries.is_empty() {
                m.cursor = 0;
            }
            Expected::Stay
        }
        7 => Expected::Stay, // Invalid rollback must do nothing.
        _ => unreachable!(),
    }
}

fn actual(h: &mut History, action: usize, urls: &[Url]) -> Expected {
    let go = match action {
        0..=2 => h.navigate(urls[action].clone()),
        3 => h.back(),
        4 => h.forward(),
        5 => {
            h.set_position(7, Some(19));
            Go::Stay
        }
        6 => {
            h.return_to(0);
            Go::Stay
        }
        7 => {
            h.return_to(usize::MAX);
            Go::Stay
        }
        _ => unreachable!(),
    };
    match go {
        Go::Stay => Expected::Stay,
        Go::Page(p) => Expected::Page(p),
        Go::Load(url) => Expected::Load(urls.iter().position(|u| u == &url).unwrap()),
        Go::Fragment(f) => Expected::Fragment(usize::from(f.is_some())),
    }
}

fn compare(h: &History, m: &Model, urls: &[Url], path: &[usize]) {
    assert_eq!(h.position(), m.cursor, "cursor after {path:?}");
    assert_eq!(h.entries().len(), m.entries.len(), "length after {path:?}");
    assert!(h.entries().len() <= MAX_HISTORY);
    for (entry, expected) in h.entries().iter().zip(&m.entries) {
        assert_eq!(entry.url, urls[expected.0], "URL after {path:?}");
        assert_eq!(
            (entry.page, entry.place),
            (expected.1, expected.2),
            "place after {path:?}"
        );
    }
    assert_eq!(h.current().is_some(), !m.entries.is_empty());
    assert_eq!(h.can_go_back(), !m.entries.is_empty() && m.cursor > 0);
    assert_eq!(h.can_go_forward(), m.cursor + 1 < m.entries.len());
}

fn walk(
    h: &History,
    m: &Model,
    urls: &[Url],
    remaining: usize,
    path: &mut Vec<usize>,
    count: &mut usize,
) {
    if remaining == 0 {
        return;
    }
    for action in 0..8 {
        let mut history = h.clone();
        let mut model = m.clone();
        path.push(action);
        assert_eq!(
            actual(&mut history, action, urls),
            apply_model(&mut model, action),
            "effect after {path:?}"
        );
        compare(&history, &model, urls, path);
        *count += 1;
        walk(&history, &model, urls, remaining - 1, path, count);
        path.pop();
    }
}

fn urls() -> Vec<Url> {
    [
        "https://model.invalid/a",
        "https://model.invalid/a#part",
        "https://model.invalid/b",
    ]
    .into_iter()
    .map(|s| Url::parse(s).unwrap())
    .collect()
}

#[test]
fn all_history_action_sequences_through_six_steps_match_the_model() {
    let mut count = 0;
    walk(
        &History::new(),
        &Model::default(),
        &urls(),
        6,
        &mut Vec::new(),
        &mut count,
    );
    assert_eq!(count, 299_592);
}

#[test]
fn history_capacity_and_branch_replacement_match_the_model() {
    let urls = urls();
    let mut h = History::new();
    let mut m = Model::default();
    for i in 0..(MAX_HISTORY * 3) {
        let action = i % 3;
        assert_eq!(actual(&mut h, action, &urls), apply_model(&mut m, action));
        actual(&mut h, 5, &urls);
        apply_model(&mut m, 5);
        compare(&h, &m, &urls, &[i]);
    }
    for i in 0..(MAX_HISTORY + 2) {
        assert_eq!(actual(&mut h, 3, &urls), apply_model(&mut m, 3));
        compare(&h, &m, &urls, &[i, 3]);
    }
    for action in [2, 0, 4, 7, 6, 4] {
        assert_eq!(actual(&mut h, action, &urls), apply_model(&mut m, action));
        compare(&h, &m, &urls, &[action]);
    }
}
