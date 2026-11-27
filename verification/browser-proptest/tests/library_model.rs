//! Independent durable-library ordering and title-prefix model.
use kobo_browser_core::library::{Library, MAX_BOOKMARKS, MAX_RECENT};
use kobo_web_document::Url;
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq)]
struct Item {
    key: u16,
    title: String,
}

fn title_prefix(title: &str) -> String {
    let mut bytes = 0;
    title
        .chars()
        .take_while(|c| {
            bytes += c.len_utf8();
            bytes <= 200
        })
        .collect()
}

fn url(key: u16) -> Url {
    Url::parse(&format!("https://library-model.invalid/{key}")).unwrap()
}
fn remember(items: &mut Vec<Item>, key: u16, title: &str, limit: usize) {
    let item = Item {
        key,
        title: title_prefix(title),
    };
    let mut next = vec![item];
    next.extend(
        items
            .iter()
            .filter(|item| item.key != key)
            .take(limit - 1)
            .cloned(),
    );
    *items = next;
}
fn check(actions: &[(bool, u16, String)]) {
    let mut actual = Library::default();
    let mut recent = vec![];
    let mut bookmarks: Vec<Item> = vec![];
    for (toggle, key, title) in actions {
        if *toggle {
            let had = bookmarks.iter().any(|item| item.key == *key);
            if had {
                bookmarks.retain(|item| item.key != *key);
            } else {
                remember(&mut bookmarks, *key, title, MAX_BOOKMARKS);
            }
            assert_eq!(actual.toggle(url(*key), title), !had);
        } else {
            remember(&mut recent, *key, title, MAX_RECENT);
            actual.visit(url(*key), title);
        }
        for (places, expected) in [(&actual.recent, &recent), (&actual.bookmarks, &bookmarks)] {
            let rows: Vec<_> = places
                .iter()
                .map(|p| Item {
                    key: p
                        .url
                        .to_string()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap(),
                    title: p.title.clone(),
                })
                .collect();
            assert_eq!(&rows, expected);
        }
        assert_eq!(
            actual.contains(&url(*key)),
            bookmarks.iter().any(|item| item.key == *key)
        );
        assert_eq!(Library::decode(&actual.encode()), actual);
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn unicode_titles_and_revisits_match_model(actions in prop::collection::vec((any::<bool>(),0u16..24,".{0,240}"),0..100)) {
        check(&actions);
    }
    #[test]
    fn capacity_and_replacement_match_model(prefix in 101u16..150, suffix in prop::collection::vec((any::<bool>(),0u16..160,".{0,20}"),0..80)) {
        let mut actions=vec![];
        for key in 0..prefix {
            actions.push((false,key,String::from("visited")));
            actions.push((true,key,String::from("bookmarked")));
        }
        actions.extend(suffix);
        check(&actions);
    }
}
