//! Editing one bounded form in the active document.
use kobo_sdk::{ActionId, DisplayMetrics, Glyph, ScreenBuilder};
use kobo_web_document::{Block, Field, Form};

use super::{action_id, fit_pages, Loaded};

pub fn forms(blocks: &[Block]) -> Vec<&Form> {
    fn walk<'a>(blocks: &'a [Block], found: &mut Vec<&'a Form>) {
        for block in blocks {
            match block {
                Block::Form(form) => found.push(form),
                Block::List { items, .. } => items.iter().for_each(|item| walk(item, found)),
                Block::Quote(inner) => walk(inner, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    walk(blocks, &mut found);
    found
}

pub fn form_mut(blocks: &mut [Block], index: usize) -> Option<&mut Form> {
    fn find<'a>(blocks: &'a mut [Block], remaining: &mut usize) -> Option<&'a mut Form> {
        for block in blocks {
            match block {
                Block::Form(form) => {
                    if *remaining == 0 {
                        return Some(form);
                    }
                    *remaining -= 1;
                }
                Block::List { items, .. } => {
                    for item in items {
                        if let Some(form) = find(item, remaining) {
                            return Some(form);
                        }
                    }
                }
                Block::Quote(inner) => {
                    if let Some(form) = find(inner, remaining) {
                        return Some(form);
                    }
                }
                _ => {}
            }
        }
        None
    }
    {
        let mut remaining = index;
        find(blocks, &mut remaining)
    }
}

fn field_title(field: &Field) -> Option<(String, String)> {
    match field {
        Field::Text {
            label,
            value,
            search,
            ..
        } => Some((
            label.clone(),
            if value.is_empty() {
                if *search { "Search words" } else { "Empty" }.to_owned()
            } else {
                value.clone()
            },
        )),
        Field::Select {
            label,
            options,
            chosen,
            radio,
            ..
        } => Some((
            label.clone(),
            chosen.and_then(|index| options.get(index)).map_or_else(
                || {
                    if *radio {
                        "None selected".to_owned()
                    } else {
                        "Choose".to_owned()
                    }
                },
                |option| option.label.clone(),
            ),
        )),
        Field::Checkbox { label, checked, .. } => Some((
            label.clone(),
            if *checked { "On" } else { "Off" }.to_owned(),
        )),
        Field::Hidden { .. } | Field::Submit { .. } => None,
    }
}

fn screen(form: &Form, entries: &[usize], page: usize, of: usize, last: bool) -> ScreenBuilder {
    let mut builder = ScreenBuilder::new("browser-form")
        .top_bar("Form")
        .top_bar_action("return", "Done")
        .secondary(format!("{} to {}", form.method_label(), form.action.host()))
        .page_turns("form-previous", "form-next")
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(of.max(1)).unwrap_or(u16::MAX),
        );
    builder = builder.rows(entries.iter().filter_map(|&index| {
        let (title, detail) = field_title(form.fields.get(index)?)?;
        Some((format!("field-{index}"), title, detail, Glyph::Terminal))
    }));
    if !last {
        return builder;
    }
    match (form.urlencoded, form.method) {
        (true, kobo_web_document::Method::Get) => builder.button("submit-form", "Submit"),
        (true, kobo_web_document::Method::Post) => {
            builder.secondary("POST is not available yet. Nothing will be sent.")
        }
        (false, _) => builder.secondary("This form's encoding is not supported."),
    }
}

trait MethodLabel {
    fn method_label(&self) -> &'static str;
}
impl MethodLabel for Form {
    fn method_label(&self) -> &'static str {
        match self.method {
            kobo_web_document::Method::Get => "GET",
            kobo_web_document::Method::Post => "POST",
        }
    }
}

pub fn form_pages(loaded: &Loaded, index: usize, metrics: &DisplayMetrics) -> Vec<Vec<usize>> {
    let Some(form) = forms(&loaded.document.blocks).get(index).copied() else {
        return Vec::new();
    };
    let entries: Vec<_> = (0..form.fields.len())
        .filter(|&i| field_title(&form.fields[i]).is_some())
        .collect();
    fit_pages(&entries, metrics, |entries| {
        screen(form, entries, 998, 999, true)
    })
}

pub fn form_screen(
    loaded: &Loaded,
    index: usize,
    page: usize,
    metrics: &DisplayMetrics,
) -> ScreenBuilder {
    let Some(form) = forms(&loaded.document.blocks).get(index).copied() else {
        return ScreenBuilder::new("browser-form")
            .top_bar("Form")
            .text("Form no longer available.");
    };
    let pages = form_pages(loaded, index, metrics);
    let page = page.min(pages.len().saturating_sub(1));
    screen(
        form,
        pages.get(page).map_or(&[][..], Vec::as_slice),
        page,
        pages.len(),
        page + 1 >= pages.len(),
    )
}

pub fn field_action(form: &Form, action: ActionId) -> Option<usize> {
    form.fields.iter().enumerate().find_map(|(index, field)| {
        (field_title(field).is_some() && action_id(&format!("field-{index}")) == action)
            .then_some(index)
    })
}

pub fn option_pages(form: &Form, field: usize, metrics: &DisplayMetrics) -> Vec<Vec<usize>> {
    let Some(Field::Select { options, .. }) = form.fields.get(field) else {
        return Vec::new();
    };
    let entries: Vec<_> = (0..options.len()).collect();
    fit_pages(&entries, metrics, |entries| {
        option_rows(form, field, entries, 998, 999)
    })
}

fn option_rows(
    form: &Form,
    field: usize,
    entries: &[usize],
    page: usize,
    of: usize,
) -> ScreenBuilder {
    let Some(Field::Select {
        label,
        options,
        chosen,
        radio,
        ..
    }) = form.fields.get(field)
    else {
        return ScreenBuilder::new("browser-options")
            .top_bar("Choose")
            .text("No choices.");
    };
    ScreenBuilder::new("browser-options")
        .top_bar(label)
        .top_bar_action("return-to-form", "Done")
        .secondary(if *radio && chosen.is_none() {
            "No choice selected"
        } else {
            "Choose one"
        })
        .page_turns("option-previous", "option-next")
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(of.max(1)).unwrap_or(u16::MAX),
        )
        .rows(entries.iter().map(|&i| {
            (
                format!("option-{i}"),
                options[i].label.clone(),
                if *chosen == Some(i) { "Selected" } else { "" }.to_owned(),
                Glyph::Circle,
            )
        }))
}

pub fn option_screen(
    form: &Form,
    field: usize,
    page: usize,
    metrics: &DisplayMetrics,
) -> ScreenBuilder {
    let pages = option_pages(form, field, metrics);
    let page = page.min(pages.len().saturating_sub(1));
    option_rows(
        form,
        field,
        pages.get(page).map_or(&[], Vec::as_slice),
        page,
        pages.len(),
    )
}

/// Successful controls, in form order, as application/x-www-form-urlencoded.
/// Submit buttons contribute only when the chosen button is known.
pub fn body(form: &Form, submit: Option<usize>) -> Option<String> {
    if !form.urlencoded {
        return None;
    }
    let mut pairs = Vec::new();
    for (index, field) in form.fields.iter().enumerate() {
        let value = match field {
            Field::Text { name, value, .. } | Field::Hidden { name, value } => {
                Some((name, value.as_str()))
            }
            Field::Checkbox {
                name,
                value,
                checked,
                ..
            } if *checked => Some((name, value.as_str())),
            Field::Select {
                name,
                options,
                chosen: Some(chosen),
                ..
            } => options
                .get(*chosen)
                .map(|option| (name, option.value.as_str())),
            Field::Submit {
                name: Some(name),
                value,
            } if submit == Some(index) => Some((name, value.as_str())),
            _ => None,
        };
        if let Some((name, value)) = value {
            if !name.is_empty() {
                pairs.push(format!(
                    "{}={}",
                    super::address::encode_query(name),
                    super::address::encode_query(value)
                ));
            }
        }
    }
    Some(pairs.join("&"))
}

pub fn get_url(form: &Form, submit: Option<usize>) -> Option<kobo_web_document::Url> {
    if form.method != kobo_web_document::Method::Get {
        return None;
    }
    let body = body(form, submit)?;
    // GET replaces the action's query, per HTML form submission; it does not
    // append to it. A failed parse means the form is not sent anywhere.
    let url = form.action.join(&format!("?{body}")).ok()?;
    (url.to_string().len() <= kobo_web_document::url::MAX_URL_LEN).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobo_web_document::{Method, OptionValue, Url};

    #[test]
    fn get_replaces_existing_query_and_encodes_unicode_and_successful_controls() {
        let form = Form {
            action: Url::parse("https://example.com/find?old=yes#top").unwrap(),
            method: Method::Get,
            urlencoded: true,
            fields: vec![
                Field::Text {
                    name: "q".into(),
                    value: "e ink & café".into(),
                    label: "Search".into(),
                    search: true,
                },
                Field::Hidden {
                    name: "source".into(),
                    value: "reader".into(),
                },
                Field::Select {
                    name: "sort".into(),
                    label: "Sort".into(),
                    options: vec![OptionValue {
                        value: "new".into(),
                        label: "New".into(),
                    }],
                    chosen: Some(0),
                    radio: false,
                },
                Field::Select {
                    name: "unused".into(),
                    label: "Unused".into(),
                    options: vec![OptionValue {
                        value: "no".into(),
                        label: "No".into(),
                    }],
                    chosen: None,
                    radio: true,
                },
                Field::Checkbox {
                    name: "only".into(),
                    value: "yes".into(),
                    label: "Only".into(),
                    checked: false,
                },
                Field::Submit {
                    name: Some("go".into()),
                    value: "Find".into(),
                },
            ],
        };
        assert_eq!(
            get_url(&form, None).unwrap().to_string(),
            "https://example.com/find?q=e+ink+%26+caf%C3%A9&source=reader&sort=new"
        );
        assert_eq!(
            get_url(&form, Some(5)).unwrap().to_string(),
            "https://example.com/find?q=e+ink+%26+caf%C3%A9&source=reader&sort=new&go=Find"
        );
    }

    #[test]
    fn post_and_unsupported_encodings_never_make_get_urls() {
        let mut form = Form {
            action: Url::parse("https://example.com/").unwrap(),
            method: Method::Post,
            urlencoded: true,
            fields: vec![Field::Text {
                name: "q".into(),
                value: "x".into(),
                label: "Search".into(),
                search: true,
            }],
        };
        assert!(get_url(&form, None).is_none());
        form.method = Method::Get;
        form.urlencoded = false;
        assert!(get_url(&form, None).is_none());
    }
}
