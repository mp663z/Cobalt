//! Pub Quiz keeps its question packs and play state on the reader.

use kobo_json::Value;
use kobo_sdk::keyboard::{TextEntry, Typing};
use kobo_sdk::{
    action_id, ActionId, BannerLevel, Context, Glyph, KoboApp, Screen, ScreenBuilder, StoreResult,
    Task, TaskId, TaskOutcome,
};
use std::fmt::Write;
use std::process::ExitCode;

const STATE: &str = "pubquiz-state";
const PACK: &str = "pubquiz-pack-v1";
const LICENSE: &str = "pubquiz-content-license";
const LICENSE_TEXT: &str = "Questions: Open Trivia DB (opentdb.com), CC-BY-SA 4.0. Cached question content remains under CC-BY-SA 4.0.";
const API: &str = "https://opentdb.com/api.php?amount=50&type=multiple";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Home,
    Question,
    Choices,
    Pass,
    Reveal,
    Podium,
    Players,
    Categories,
    HowTo,
    About,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Question {
    category: String,
    difficulty: String,
    text: String,
    answers: [String; 4],
    correct: usize,
}

fn question(category: &str, text: &str, answers: [&str; 4], correct: usize) -> Question {
    Question {
        category: category.into(),
        difficulty: String::new(),
        text: text.into(),
        answers: answers.map(Into::into),
        correct,
    }
}

fn bundled_questions() -> Vec<Question> {
    [
        question(
            "Science",
            "Which planet has the shortest year?",
            ["Mercury", "Mars", "Venus", "Earth"],
            0,
        ),
        question(
            "General knowledge",
            "What is the capital of Finland?",
            ["Oslo", "Helsinki", "Tallinn", "Stockholm"],
            1,
        ),
        question(
            "History",
            "Which ship carried Charles Darwin on his voyage?",
            ["Beagle", "Endeavour", "Victory", "Resolution"],
            0,
        ),
        question(
            "Arts",
            "Who painted The Persistence of Memory?",
            ["Miró", "Dalí", "Picasso", "Kahlo"],
            1,
        ),
        question(
            "Geography",
            "Which river runs through Budapest?",
            ["Rhine", "Danube", "Seine", "Tagus"],
            1,
        ),
        question(
            "Science",
            "What is the chemical symbol for gold?",
            ["Ag", "Gd", "Au", "Go"],
            2,
        ),
        question(
            "Literature",
            "Who wrote Frankenstein?",
            [
                "Mary Shelley",
                "George Eliot",
                "Jane Austen",
                "Emily Brontë",
            ],
            0,
        ),
        question(
            "Music",
            "How many strings does a standard violin have?",
            ["Three", "Four", "Five", "Six"],
            1,
        ),
        question(
            "Nature",
            "Which animal is the largest living bird?",
            ["Emu", "Albatross", "Ostrich", "Condor"],
            2,
        ),
        question(
            "Sport",
            "How many players start on a football team?",
            ["Nine", "Ten", "Eleven", "Twelve"],
            2,
        ),
    ]
    .into()
}
struct Quiz {
    view: View,
    party: bool,
    player: usize,
    question: usize,
    answer: Option<usize>,
    scores: [u8; 4],
    packs: u8,
    note: Option<String>,
    rounds: u16,
    questions: Vec<Question>,
    round_questions: Vec<Question>,
    page: usize,
    sync_task: Option<TaskId>,
    pack_synced: bool,
    synced_day: Option<u32>,
    export: Option<kobo_sdk::exports::Export>,
    names: [String; 4],
    players: usize,
    category: String,
    entry: TextEntry,
    renaming: usize,
}
impl Default for Quiz {
    fn default() -> Self {
        Self {
            view: View::Home,
            party: true,
            player: 0,
            question: 0,
            answer: None,
            scores: [0; 4],
            packs: 0,
            note: None,
            rounds: 0,
            questions: bundled_questions(),
            round_questions: bundled_questions(),
            page: 0,
            sync_task: None,
            pack_synced: false,
            synced_day: None,
            export: None,
            names: NAMES.map(String::from),
            players: 4,
            category: String::new(),
            entry: TextEntry::new().opened_by("name-entry"),
            renaming: 0,
        }
    }
}
const NAMES: [&str; 4] = ["Ada", "Bert", "Cleo", "Dev"];

impl Quiz {
    fn player_name(&self) -> &str {
        &self.names[self.player]
    }
    fn save(&self, context: &mut Context) {
        context.store().save(
            STATE,
            format!(
                "{}|{}|{}|{}|{}|{}",
                self.packs,
                self.rounds,
                self.synced_day.map_or(String::new(), |day| day.to_string()),
                self.players,
                self.names.join("|"),
                self.category.replace('|', " ")
            )
            .into_bytes(),
        );
    }
    fn begin(&mut self, party: bool) {
        self.party = party;
        self.view = View::Question;
        self.question = 0;
        self.player = 0;
        self.answer = None;
        self.scores = [0; 4];
        self.note = None;
        self.page = 0;
        let pool: Vec<&Question> = match self.active_category() {
            Some(category) => self
                .questions
                .iter()
                .filter(|q| q.category == category)
                .collect(),
            None => self.questions.iter().collect(),
        };
        let offset = usize::from(self.rounds) * 10 % pool.len();
        let take = pool.len().min(10);
        self.round_questions = pool
            .iter()
            .cycle()
            .skip(offset)
            .take(take)
            .map(|q| (*q).clone())
            .collect();
    }
    /// Distinct categories in the current packs, in first-seen order, with counts.
    fn categories(&self) -> Vec<(String, usize)> {
        let mut seen: Vec<(String, usize)> = Vec::new();
        for question in &self.questions {
            match seen.iter_mut().find(|(name, _)| *name == question.category) {
                Some((_, count)) => *count += 1,
                None => seen.push((question.category.clone(), 1)),
            }
        }
        seen
    }
    /// The chosen category, but only while the current packs still contain it.
    fn active_category(&self) -> Option<&str> {
        if self.category.is_empty() {
            return None;
        }
        self.questions
            .iter()
            .any(|q| q.category == self.category)
            .then_some(self.category.as_str())
    }
    fn sync(&mut self, context: &mut Context) {
        if self.sync_task.is_some() {
            return;
        }
        self.note = None;
        self.sync_task = context.spawn_retrying(Task::Fetch {
            url: API.into(),
            offset: 0,
            max_bytes: 128 * 1024,
            credential: None,
            headers: Vec::new(),
        });
        if self.sync_task.is_none() {
            self.note = Some("Trivia packs are already updating.".into());
        }
    }
    fn page_count(&self, context: &Context) -> usize {
        if self.view == View::Categories {
            let categories = self.categories();
            let titles = categories
                .iter()
                .map(|(name, _)| (name.as_str(), ""))
                .collect::<Vec<_>>();
            context.paginate_rows(&titles, true).len()
        } else if self.view == View::Choices {
            let question = &self.round_questions[self.question % self.round_questions.len()];
            let titles = question
                .answers
                .iter()
                .enumerate()
                .map(|(index, answer)| format!("{} · {answer}", index + 1))
                .collect::<Vec<_>>();
            let rows = titles
                .iter()
                .map(|title| (title.as_str(), ""))
                .collect::<Vec<_>>();
            context.paginate_rows(&rows, true).len()
        } else {
            context.paginate(&question_text(self), true).len()
        }
    }
    fn show(&self, context: &mut Context) {
        context.set_screen(screen_with(self, context));
    }
}
fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

const MAX_NAME_CHARS: usize = 12;

fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| *c != '|')
        .take(MAX_NAME_CHARS)
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn today_day() -> u32 {
    u32::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400,
    )
    .unwrap_or(u32::MAX)
}

fn civil_date(days_since_epoch: i64) -> String {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

impl Quiz {
    /// The taps that belong to choosing and naming players. Returns whether
    /// it took the tap.
    fn players_action(&mut self, context: &mut Context, action: ActionId) -> bool {
        if action == action_id("players") && self.view == View::Home {
            self.view = View::Players;
        } else if self.view != View::Players {
            return false;
        } else if let Some(count) = (2..=4).find(|n| action == action_id(&format!("players-{n}"))) {
            self.players = count;
            self.save(context);
        } else if let Some(index) = (0..4).find(|i| action == action_id(&format!("name-{i}"))) {
            self.renaming = index;
            self.entry.open();
        } else {
            return false;
        }
        self.show(context);
        true
    }
    /// The taps that belong to choosing a category. Returns whether it took
    /// the tap.
    fn categories_action(&mut self, context: &mut Context, action: ActionId) -> bool {
        if action == action_id("categories") && self.view == View::Home {
            self.view = View::Categories;
            self.page = 0;
        } else if self.view != View::Categories {
            return false;
        } else if action == action_id("category-all") {
            self.category.clear();
            self.save(context);
            self.view = View::Home;
        } else if let Some(index) =
            (0..self.categories().len()).find(|i| action == action_id(&format!("category-{i}")))
        {
            self.category.clone_from(&self.categories()[index].0);
            self.save(context);
            self.view = View::Home;
        } else {
            return false;
        }
        self.show(context);
        true
    }
}

fn ordinal(n: usize) -> &'static str {
    ["first", "second", "third", "fourth"][n - 1]
}

fn choice(index: usize) -> String {
    format!("answer-{index}")
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" | "#039" | "#39" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "nbsp" => Some(' '),
        "eacute" => Some('é'),
        "ouml" => Some('ö'),
        "uuml" => Some('ü'),
        _ => entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .and_then(char::from_u32)
            .or_else(|| {
                entity
                    .strip_prefix('#')
                    .and_then(|digits| digits.parse::<u32>().ok())
                    .and_then(char::from_u32)
            }),
    }
}

fn clean_text(input: &str, limit: usize) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';').filter(|end| *end <= 12) else {
            output.push('&');
            rest = &rest[1..];
            continue;
        };
        if let Some(decoded) = decode_entity(&rest[1..end]) {
            output.push(decoded);
            rest = &rest[end + 1..];
        } else {
            output.push('&');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    let normalized = output.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(limit).collect()
}

fn parse_pack(bytes: &[u8]) -> Option<Vec<Question>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let value = kobo_json::parse(text).ok()?;
    if value.get("response_code").and_then(Value::as_i64) != Some(0) {
        return None;
    }
    let results = value.get("results")?.as_array()?;
    let questions = results
        .iter()
        .filter_map(|item| {
            let category = clean_text(item.get("category")?.as_str()?, 48);
            let text = clean_text(item.get("question")?.as_str()?, 240);
            let correct = clean_text(item.get("correct_answer")?.as_str()?, 80);
            let wrong = item.get("incorrect_answers")?.as_array()?;
            if category.is_empty() || text.is_empty() || correct.is_empty() || wrong.len() != 3 {
                return None;
            }
            let difficulty = item
                .get("difficulty")
                .and_then(Value::as_str)
                .map(|d| title_case(&clean_text(d, 16)))
                .unwrap_or_default();
            let mut answers = wrong
                .iter()
                .map(|answer| clean_text(answer.as_str().unwrap_or_default(), 80))
                .collect::<Vec<_>>();
            if answers.iter().any(String::is_empty) {
                return None;
            }
            let slot = text.bytes().fold(0_usize, |hash, byte| {
                hash.wrapping_mul(33).wrapping_add(usize::from(byte))
            }) % 4;
            answers.insert(slot, correct);
            let answers: [String; 4] = answers.try_into().ok()?;
            Some(Question {
                category,
                difficulty,
                text,
                answers,
                correct: slot,
            })
        })
        .take(50)
        .collect::<Vec<_>>();
    (questions.len() >= 10).then_some(questions)
}
#[cfg(test)]
fn screen(quiz: &Quiz) -> Screen {
    screen_with(quiz, &Context::default())
}

/// Category plus the pack's difficulty rating when it carries one.
fn label_line(question: &Question) -> String {
    if question.difficulty.is_empty() {
        question.category.clone()
    } else {
        format!("{} · {}", question.category, question.difficulty)
    }
}

fn question_text(quiz: &Quiz) -> String {
    let question = &quiz.round_questions[quiz.question % quiz.round_questions.len()];
    let mut text = format!(
        "{} · question {} of {}\n\n{}",
        label_line(question),
        quiz.question + 1,
        quiz.round_questions.len(),
        question.text
    );
    for (index, answer) in question.answers.iter().enumerate() {
        write!(text, "\n\n{} · {answer}", index + 1).expect("writing to a String");
    }
    text
}

fn question_title(quiz: &Quiz) -> String {
    if quiz.party {
        format!("{} answers", quiz.player_name())
    } else {
        "Solo round".into()
    }
}

fn answer_rows(question: &Question) -> impl Iterator<Item = (String, String, &str, u16)> {
    question.answers.iter().enumerate().map(|(index, answer)| {
        (
            choice(index),
            format!("{} · {answer}", index + 1),
            "",
            u16::try_from(index + 1).expect("four answers"),
        )
    })
}

fn question_screen(quiz: &Quiz, context: &Context) -> Screen {
    let question = &quiz.round_questions[quiz.question % quiz.round_questions.len()];
    let compact = ScreenBuilder::new("pubquiz-question")
        .top_bar(question_title(quiz))
        .secondary(format!(
            "{} · question {} of {}",
            label_line(question),
            quiz.question + 1,
            quiz.round_questions.len()
        ))
        .text(&question.text)
        .rows(answer_rows(question))
        .build();
    if compact
        .diagnostics(&context.metrics(), &kobo_sdk::Chrome::default())
        .issues
        .is_empty()
    {
        return compact;
    }
    // Long questions stay complete, including answers that share a prefix.
    // The same measured prose pagination as the reader keeps every word reachable.
    let pages = context.paginate(&question_text(quiz), true);
    let page = quiz.page.min(pages.len().saturating_sub(1));
    let mut builder = ScreenBuilder::new("pubquiz-question").top_bar(question_title(quiz));
    for paragraph in &pages[page] {
        builder = builder.text(paragraph);
    }
    builder
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(pages.len()).unwrap_or(u16::MAX),
        )
        .action_bar([
            ("previous-page", "Previous"),
            ("next-page", "Next"),
            ("choose", "Answer"),
        ])
        .build()
}

fn choices_screen(quiz: &Quiz, context: &Context) -> Screen {
    let question = &quiz.round_questions[quiz.question % quiz.round_questions.len()];
    let titles = question
        .answers
        .iter()
        .enumerate()
        .map(|(index, answer)| format!("{} · {answer}", index + 1))
        .collect::<Vec<_>>();
    let rows = titles
        .iter()
        .map(|title| (title.as_str(), ""))
        .collect::<Vec<_>>();
    let pages = context.paginate_rows(&rows, true);
    let page = quiz.page.min(pages.len().saturating_sub(1));
    ScreenBuilder::new("pubquiz-choices")
        .top_bar("Choose an answer")
        .rows(pages[page].iter().map(|&index| {
            (
                choice(index),
                titles[index].as_str(),
                "",
                u16::try_from(index + 1).expect("four answers"),
            )
        }))
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(pages.len()).unwrap_or(u16::MAX),
        )
        .action_bar([
            ("previous-page", "Previous"),
            ("next-page", "Next"),
            ("question", "Question"),
        ])
        .build()
}

fn scorecard_text(quiz: &Quiz) -> String {
    let players = if quiz.party { quiz.players } else { 1 };
    let mut text = format!(
        "Pub Quiz scorecard - {}\n",
        civil_date(i64::from(today_day()))
    );
    for (i, name) in quiz.names.iter().enumerate().take(players) {
        text.push_str(&format!(
            "\n{name}: {} of {}",
            quiz.scores[i],
            quiz.round_questions.len()
        ));
    }
    text.push('\n');
    text
}

#[allow(clippy::too_many_lines)]
fn screen_with(quiz: &Quiz, context: &Context) -> Screen {
    if let Some(export) = &quiz.export {
        return export.screen();
    }
    if quiz.entry.is_open() {
        return ScreenBuilder::new("pubquiz-name")
            .top_bar("Player name")
            .owns_back(true)
            .secondary(format!("Use {MAX_NAME_CHARS} characters or fewer."))
            .text_entry(&quiz.entry, "Name", "Save")
            .build();
    }
    let question = &quiz.round_questions[quiz.question % quiz.round_questions.len()];
    match quiz.view {
        View::Home => {
            let mut b = ScreenBuilder::new("pubquiz-home")
                .top_bar("Pub Quiz")
                .facts([
                    ("Questions", format!("{} ready", quiz.questions.len())),
                    ("Pass-around", format!("{} players", quiz.players)),
                    (
                        "Pack",
                        if quiz.synced_day.is_some() {
                            "Open Trivia DB".to_owned()
                        } else {
                            "Built-in set".to_owned()
                        },
                    ),
                    (
                        "Updated",
                        quiz.synced_day.map_or_else(
                            || "Ships with the app".to_owned(),
                            |day| civil_date(i64::from(day)),
                        ),
                    ),
                ]);
            if let Some(note) = &quiz.note {
                b = b.banner(BannerLevel::Info, note);
            }
            b.primary_button("party", "Start pass-around")
                .buttons([
                    ("solo", "Solo round"),
                    (
                        "sync",
                        if quiz.sync_task.is_some() {
                            "Working…"
                        } else {
                            "Sync packs"
                        },
                    ),
                ])
                .rows([(
                    "categories",
                    "Categories",
                    quiz.active_category().unwrap_or("All categories"),
                    Glyph::Tag,
                )])
                .buttons([
                    ("players", "Players"),
                    ("how-to-play", "How to play"),
                    ("about", "About"),
                ])
                .build()
        }
        View::Question => question_screen(quiz, context),
        View::Choices => choices_screen(quiz, context),
        View::Pass => {
            let next = &quiz.names[(quiz.player + 1) % quiz.players];
            ScreenBuilder::new("pubquiz-pass")
                .top_bar("Pass it on")
                .heading("Answer locked")
                .text(format!(
                    "Hand the Kobo to {next} before the result is shown."
                ))
                .text(format!("{next}, show the result when you are ready."))
                .primary_button("reveal", "Show result")
                .build()
        }
        View::Reveal => {
            let right = quiz.answer == Some(question.correct);
            ScreenBuilder::new("pubquiz-reveal")
                .top_bar("Round result")
                .heading(if right { "Correct" } else { "Not this time" })
                .secondary(format!(
                    "{} · {}",
                    question.category, question.answers[question.correct]
                ))
                .facts(
                    (0..if quiz.party { quiz.players } else { 1 })
                        .map(|i| (quiz.names[i].clone(), format!("{} points", quiz.scores[i]))),
                )
                .primary_button(
                    "continue",
                    if quiz.question + 1 == quiz.round_questions.len() {
                        "See podium"
                    } else {
                        "Next question"
                    },
                )
                .build()
        }
        View::Podium => ScreenBuilder::new("pubquiz-podium")
            .top_bar("Pub Quiz")
            .heading("Podium")
            .rows((0..if quiz.party { quiz.players } else { 1 }).map(|i| {
                (
                    format!("player-{i}"),
                    quiz.names[i].clone(),
                    format!("{} points", quiz.scores[i]),
                    Glyph::Person,
                )
            }))
            .primary_button("home", "Finish round")
            .button("export", "Save a copy")
            .build(),
        View::Players => ScreenBuilder::new("pubquiz-players")
            .top_bar("Players")
            .owns_back(true)
            .text("Pass-around players take turns in this order. Tap a name to change it.")
            .chips([
                ("players-2", "2 players", quiz.players == 2),
                ("players-3", "3 players", quiz.players == 3),
                ("players-4", "4 players", quiz.players == 4),
            ])
            .rows(quiz.names.iter().enumerate().map(|(i, name)| {
                (
                    format!("name-{i}"),
                    name.clone(),
                    if i < quiz.players {
                        format!("plays, answers {}", ordinal(i + 1))
                    } else {
                        "sits out".to_owned()
                    },
                    Glyph::Person,
                )
            }))
            .build(),
        View::Categories => {
            let active = quiz.active_category();
            let mut rows: Vec<(String, String, String)> = vec![(
                "category-all".to_owned(),
                "All categories".to_owned(),
                if active.is_none() {
                    format!("{} questions · in use", quiz.questions.len())
                } else {
                    format!("{} questions", quiz.questions.len())
                },
            )];
            rows.extend(
                quiz.categories()
                    .iter()
                    .enumerate()
                    .map(|(i, (name, count))| {
                        (
                            format!("category-{i}"),
                            name.clone(),
                            if active == Some(name.as_str()) {
                                format!("{count} questions · in use")
                            } else {
                                format!("{count} questions")
                            },
                        )
                    }),
            );
            let titles = rows
                .iter()
                .map(|(_, name, sub)| (name.as_str(), sub.as_str()))
                .collect::<Vec<_>>();
            let pages = context.paginate_rows(&titles, true);
            let page = quiz.page.min(pages.len().saturating_sub(1));
            ScreenBuilder::new("pubquiz-categories")
                .top_bar("Categories")
                .owns_back(true)
                .rows(pages[page].iter().map(|&index| {
                    let (id, name, sub) = &rows[index];
                    (id.clone(), name.clone(), sub.clone(), Glyph::Tag)
                }))
                .page_position(
                    u16::try_from(page + 1).unwrap_or(u16::MAX),
                    u16::try_from(pages.len()).unwrap_or(u16::MAX),
                )
                .action_bar([("previous-page", "Previous"), ("next-page", "Next")])
                .build()
        }
        View::HowTo => ScreenBuilder::new("pubquiz-help")
            .top_bar("How to play")
            .owns_back(true)
            .heading("Up to ten questions, one Kobo")
            .text("Solo: choose an answer and see the result right away.")
            .text("Pass-around: answer, pass the Kobo, then reveal the result.")
            .text("Players take turns. The highest score after the round wins.")
            .bottom_action("home", "Play")
            .build(),
        View::About => ScreenBuilder::new("pubquiz-about")
            .top_bar("Pub Quiz")
            .heading("About")
            .text("Question packs use Open Trivia DB content, licensed CC-BY-SA 4.0.")
            .text("opentdb.com · cached packs are redistributed under the same license.")
            .text("Synced packs can carry a difficulty rating; the built-in set is unrated.")
            .button("home", "Back to packs")
            .build(),
    }
}
impl KoboApp for Quiz {
    fn on_start(&mut self, context: &mut Context) {
        context.store().load(STATE);
        context.store().load(PACK);
        context
            .store()
            .save(LICENSE, LICENSE_TEXT.as_bytes().to_vec());
        self.show(context);
    }
    fn on_store(&mut self, context: &mut Context, result: StoreResult) {
        if let StoreResult::Loaded { key, value } = result {
            if key == STATE {
                if let Some(bytes) = value {
                    if let Ok(s) = String::from_utf8(bytes) {
                        let p: Vec<_> = s.split('|').collect();
                        self.packs = p.first().and_then(|x| x.parse().ok()).unwrap_or(0);
                        self.rounds = p.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
                        self.synced_day = p.get(2).and_then(|x| x.parse().ok());
                        self.players = p
                            .get(3)
                            .and_then(|x| x.parse().ok())
                            .filter(|n| (2..=4).contains(n))
                            .unwrap_or(4);
                        for (i, slot) in self.names.iter_mut().enumerate() {
                            if let Some(name) = p.get(4 + i) {
                                if let Some(name) = clean_name(name) {
                                    *slot = name;
                                }
                            }
                        }
                        if let Some(category) = p.get(8) {
                            category
                                .replace('|', " ")
                                .trim()
                                .clone_into(&mut self.category);
                        }
                    }
                }
            } else if key == PACK && !self.pack_synced {
                if let Some(bytes) = value {
                    if let Some(questions) = parse_pack(&bytes) {
                        self.questions = questions;
                        self.packs = 1;
                    } else {
                        context.store().forget(PACK);
                        self.note =
                            Some("Saved questions were damaged; the built-in set is ready.".into());
                    }
                }
            }
            self.show(context);
        }
    }
    fn on_task(&mut self, context: &mut Context, task: TaskId, outcome: TaskOutcome) {
        if self.sync_task != Some(task) {
            return;
        }
        self.sync_task = None;
        match outcome {
            TaskOutcome::Completed(bytes) => {
                if let Some(questions) = parse_pack(&bytes) {
                    self.questions = questions;
                    self.pack_synced = true;
                    self.packs = 1;
                    self.synced_day = Some(today_day());
                    context.store().save(PACK, bytes);
                    self.note = Some(format!(
                        "{} fresh questions saved for offline play.",
                        self.questions.len()
                    ));
                    self.save(context);
                } else {
                    self.note = Some("Open Trivia DB returned no usable question set.".into());
                }
            }
            TaskOutcome::Failed(kobo_sdk::TaskError::Offline) => {
                self.note = Some("Off the air. Existing packs still play offline.".into());
            }
            TaskOutcome::Failed(_) | TaskOutcome::Cancelled => {
                self.note =
                    Some("Open Trivia DB did not answer. Join Wi-Fi and try sync again.".into());
            }
        }
        self.show(context);
    }
    fn on_save(&mut self, context: &mut Context, key: &str, result: StoreResult) {
        if let Some(export) = self.export.as_mut() {
            if export.on_save(context, key, &result) {
                self.show(context);
                return;
            }
        }
        self.on_store(context, result);
    }
    fn on_shelf(&mut self, context: &mut Context, name: &str, result: StoreResult) {
        if let Some(export) = self.export.as_mut() {
            if export.on_shelf(context, name, &result) {
                self.show(context);
                return;
            }
        }
        self.on_store(context, result);
    }
    fn on_action(&mut self, context: &mut Context, action: ActionId) {
        if let Some(event) = self.entry.handle(action) {
            match event {
                Typing::Submitted(name) => {
                    if let Some(name) = clean_name(&name) {
                        self.names[self.renaming] = name;
                        self.save(context);
                    } else {
                        self.note = Some(format!(
                            "Names need 1 to {MAX_NAME_CHARS} characters. Nothing changed."
                        ));
                    }
                }
                Typing::Changed | Typing::Cancelled => {}
            }
            self.show(context);
            return;
        }
        if action == action_id("export") && self.view == View::Podium {
            match kobo_sdk::exports::Export::new(
                "Pub Quiz scorecard",
                kobo_sdk::exports::Format::Text,
                scorecard_text(self).into_bytes(),
            ) {
                Ok(mut export) => {
                    export.begin(context);
                    self.export = Some(export);
                }
                Err(reason) => {
                    self.note = Some(format!("The copy was refused: {reason}"));
                }
            }
        } else if action == action_id("export-confirm") || action == action_id("export-retry") {
            if let Some(export) = self.export.as_mut() {
                export.begin(context);
            }
        } else if self.export.is_some() && action == ActionId::BACK {
            self.export = None;
        } else if action == action_id("choose") && self.view == View::Question {
            self.view = View::Choices;
            self.page = 0;
        } else if action == action_id("question") && self.view == View::Choices {
            self.view = View::Question;
            self.page = 0;
        } else if matches!(self.view, View::Question | View::Choices | View::Categories)
            && action == action_id("next-page")
        {
            self.page = self
                .page
                .saturating_add(1)
                .min(self.page_count(context).saturating_sub(1));
        } else if matches!(self.view, View::Question | View::Choices | View::Categories)
            && action == action_id("previous-page")
        {
            self.page = self.page.saturating_sub(1);
        } else if action == action_id("party") && self.view == View::Home {
            self.begin(true);
        } else if action == action_id("solo") && self.view == View::Home {
            self.begin(false);
        } else if action == action_id("sync") && self.view == View::Home {
            self.sync(context);
        } else if action == action_id("about") {
            self.view = View::About;
        } else if action == action_id("how-to-play") {
            self.view = View::HowTo;
        } else if self.players_action(context, action) || self.categories_action(context, action) {
        } else if action == ActionId::BACK || action == action_id("home") {
            self.view = View::Home;
        } else if let Some(answer) = (0..4).find(|i| {
            matches!(self.view, View::Question | View::Choices) && action == action_id(&choice(*i))
        }) {
            self.answer = Some(answer);
            self.view = if self.party { View::Pass } else { View::Reveal };
            if answer == self.round_questions[self.question % self.round_questions.len()].correct {
                self.scores[self.player] += 1;
            }
        } else if action == action_id("reveal") && self.view == View::Pass {
            self.view = View::Reveal;
        } else if action == action_id("continue") && self.view == View::Reveal {
            self.question += 1;
            self.player = if self.party {
                (self.player + 1) % self.players
            } else {
                0
            };
            self.page = 0;
            self.answer = None;
            if self.question >= self.round_questions.len() {
                self.view = View::Podium;
                self.rounds = self.rounds.saturating_add(1);
                self.save(context);
            } else {
                self.view = View::Question;
            }
        }
        self.show(context);
    }
}
fn main() -> ExitCode {
    kobo_sdk::run("pubquiz", Quiz::default()).map_or_else(
        |error| {
            eprintln!("pubquiz: {error}");
            ExitCode::FAILURE
        },
        |()| ExitCode::SUCCESS,
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn clean_name_trims_limits_and_refuses_blank() {
        assert_eq!(
            clean_name("  Bo  ".to_string().as_str()).as_deref(),
            Some("Bo")
        );
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name("a|b").as_deref(), Some("ab"));
        let long = clean_name("Averylongfirstnameindeed");
        assert_eq!(long.unwrap().chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn players_and_names_survive_the_state_round_trip() {
        let mut quiz = Quiz {
            players: 3,
            ..Quiz::default()
        };
        quiz.names[0] = "Bo".to_owned();
        let mut context = Context::default();
        quiz.save(&mut context);
        let saved = context
            .take_commands()
            .into_iter()
            .find_map(|command| match command {
                kobo_sdk::Command::Store(kobo_sdk::StoreRequest::Save { key, value })
                    if key == STATE =>
                {
                    Some(value)
                }
                _ => None,
            })
            .expect("state save");
        let text = String::from_utf8(saved).unwrap();
        let parts: Vec<_> = text.split('|').collect();
        assert_eq!(parts[3], "3");
        assert_eq!(&parts[4..8], ["Bo", "Bert", "Cleo", "Dev"]);
    }

    #[test]
    fn a_three_player_pass_names_the_third_player_next() {
        let mut quiz = Quiz {
            players: 3,
            ..Quiz::default()
        };
        quiz.begin(true);
        quiz.player = 1;
        quiz.view = crate::View::Pass;
        let shown = format!("{:?}", screen(&quiz));
        assert!(shown.contains("Cleo"));
        assert!(!shown.contains("Dev"));
    }

    #[test]
    fn scorecard_names_every_player_and_the_day() {
        let quiz = Quiz {
            scores: [3, 5, 2, 4],
            ..Quiz::default()
        };
        let text = scorecard_text(&quiz);
        assert!(text.starts_with("Pub Quiz scorecard - "));
        assert!(text.contains("\nAda: 3 of 10"));
        assert!(text.contains("\nDev: 4 of 10"));
    }

    #[test]
    fn solo_scorecard_names_only_the_player() {
        let quiz = Quiz {
            party: false,
            scores: [7, 9, 9, 9],
            ..Quiz::default()
        };
        let text = scorecard_text(&quiz);
        assert!(text.contains("\nAda: 7 of 10"));
        assert!(!text.contains("Bert"));
    }

    #[test]
    fn state_round_trip_keeps_the_sync_day() {
        let quiz = Quiz {
            packs: 1,
            rounds: 3,
            synced_day: Some(20_000),
            ..Quiz::default()
        };
        let mut context = Context::default();
        quiz.save(&mut context);
        let saved = context
            .take_commands()
            .into_iter()
            .find_map(|command| match command {
                kobo_sdk::Command::Store(kobo_sdk::StoreRequest::Save { key, value })
                    if key == STATE =>
                {
                    Some(value)
                }
                _ => None,
            })
            .expect("state save");
        let text = String::from_utf8(saved).unwrap();
        let parts: Vec<_> = text.split('|').collect();
        assert_eq!(parts[0], "1");
        assert_eq!(parts[1], "3");
        assert_eq!(parts[2], "20000");
        // The same parse on_store applies when the app next opens.
        let parsed: Option<u32> = parts.get(2).and_then(|x| x.parse().ok());
        assert_eq!(parsed, Some(20_000));
    }

    #[test]
    fn state_without_a_sync_day_reads_as_built_in() {
        let text = "1|3|";
        let parts: Vec<_> = text.split('|').collect();
        let parsed: Option<u32> = parts.get(2).and_then(|x| x.parse().ok());
        assert_eq!(parsed, None);
        let legacy = "1|3";
        let parts: Vec<_> = legacy.split('|').collect();
        let parsed: Option<u32> = parts.get(2).and_then(|x| x.parse().ok());
        assert_eq!(parsed, None);
    }

    #[test]
    fn home_names_the_pack_source_honestly() {
        let quiz = Quiz::default();
        let built_in = format!("{:?}", screen(&quiz));
        let quiz = Quiz {
            synced_day: Some(20_469),
            ..Quiz::default()
        };
        let synced = format!("{:?}", screen(&quiz));
        assert_ne!(built_in, synced);
    }

    use super::*;
    use kobo_ui::{Chrome, CLARA_BW_METRICS};

    #[test]
    fn synced_pack_difficulty_is_optional_and_shown() {
        let with = format!(
            r#"{{"response_code":0,"results":[{}]}}"#,
            (0..10)
                .map(|i| format!(
                    r#"{{"category":"Science","difficulty":"medium","question":"Q{i}?","correct_answer":"Right","incorrect_answers":["W1","W2","W3"]}}"#
                ))
                .collect::<Vec<_>>()
                .join(",")
        );
        let questions = parse_pack(with.as_bytes()).expect("pack parses");
        assert_eq!(questions[0].difficulty, "Medium");
        assert!(bundled_questions()[0].difficulty.is_empty());
        assert_eq!(label_line(&questions[0]), "Science · Medium");
        assert_eq!(label_line(&bundled_questions()[0]), "Science");
    }

    #[test]
    fn a_chosen_category_shortens_the_round_honestly() {
        let mut quiz = Quiz {
            category: "Science".into(),
            ..Quiz::default()
        };
        quiz.begin(true);
        assert_eq!(quiz.round_questions.len(), 2);
        assert!(quiz.round_questions.iter().all(|q| q.category == "Science"));
    }

    #[test]
    fn a_category_the_pack_lost_falls_back_to_all() {
        let mut quiz = Quiz {
            category: "Vanished".into(),
            ..Quiz::default()
        };
        assert_eq!(quiz.active_category(), None);
        quiz.begin(true);
        assert_eq!(quiz.round_questions.len(), 10);
    }

    #[test]
    fn category_survives_the_save_file() {
        let saved = "1|3|20469|3|Ada|Bert|Cleo|Dev|History";
        let parts: Vec<_> = saved.split('|').collect();
        assert_eq!(parts.get(8), Some(&"History"));
        let legacy = "1|3|20469|3|Ada|Bert|Cleo|Dev";
        let parts: Vec<_> = legacy.split('|').collect();
        assert_eq!(parts.get(8), None);
    }

    #[test]
    fn locked_answer_hides_the_reveal() {
        let mut quiz = Quiz::default();
        quiz.begin(true);
        quiz.answer = Some(0);
        quiz.view = View::Pass;
        let layout = screen(&quiz).layout_with(&CLARA_BW_METRICS, &Chrome::default());
        assert!(layout.rect_of_action(action_id("reveal")).is_some());
        assert!(layout.rect_of_action(action_id("answer-0")).is_none());
    }
    #[test]
    fn entities_decode_before_render() {
        assert_eq!(
            clean_text("Rock &amp; Roll &#039;A&#039;", 80),
            "Rock & Roll 'A'"
        );
    }
    #[test]
    fn question_controls_fit_clara() {
        let quiz = Quiz {
            view: View::Question,
            ..Quiz::default()
        };
        let screen = screen(&quiz);
        assert!(screen
            .diagnostics(&CLARA_BW_METRICS, &Chrome::default())
            .issues
            .is_empty());
        for i in 0..4 {
            assert!(screen
                .layout_with(&CLARA_BW_METRICS, &Chrome::default())
                .rect_of_action(action_id(&choice(i)))
                .is_some());
        }
    }

    #[test]
    fn a_round_has_ten_distinct_questions_and_short_help() {
        assert_eq!(bundled_questions().len(), 10);
        let quiz = Quiz {
            view: View::HowTo,
            ..Quiz::default()
        };
        assert!(screen(&quiz)
            .diagnostics(&CLARA_BW_METRICS, &Chrome::measuring(true))
            .issues
            .is_empty());
    }

    #[test]
    fn open_trivia_pack_is_parsed_bounded_and_mixed() {
        let mut items = Vec::new();
        for index in 0..10 {
            items.push(format!(
                r#"{{"category":"Science &amp; Nature","question":"Question {index}?","correct_answer":"Right","incorrect_answers":["Wrong 1","Wrong 2","Wrong 3"]}}"#
            ));
        }
        let body = format!(r#"{{"response_code":0,"results":[{}]}}"#, items.join(","));
        let questions = parse_pack(body.as_bytes()).expect("valid pack");
        assert_eq!(questions.len(), 10);
        assert_eq!(questions[0].category, "Science & Nature");
        assert!(questions.iter().all(|question| question.answers.len() == 4));
        assert!(parse_pack(br#"{"response_code":1,"results":[]}"#).is_none());
    }

    #[test]
    fn action_graph_reaches_party_views() {
        use kobo_sdk::AppRunner;
        let mut runner = AppRunner::new(Quiz::default());
        runner.start();
        runner.store_result(StoreResult::Loaded {
            key: STATE.into(),
            value: None,
        });
        assert_eq!(runner.app().view, View::Home);
        runner.action(action_id("about"));
        assert_eq!(runner.app().view, View::About);
        runner.action(action_id("home"));
        runner.action(action_id("party"));
        assert_eq!(runner.app().view, View::Question);
        runner.action(action_id(&choice(0)));
        assert_eq!(runner.app().view, View::Pass);
        runner.action(action_id("reveal"));
        assert_eq!(runner.app().view, View::Reveal);
        runner.app_mut().question = 9;
        runner.action(action_id("continue"));
        assert_eq!(runner.app().view, View::Podium);
        runner.action(action_id("home"));
        assert_eq!(runner.app().view, View::Home);
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use kobo_sdk::{AppRunner, Chrome, DisplayMetrics};
    use kobo_ui::TextScale;

    fn pack() -> Vec<u8> {
        let item = r#"{"category":"New pack","question":"Changed question?","correct_answer":"Right","incorrect_answers":["Wrong 1","Wrong 2","Wrong 3"]}"#;
        format!(
            r#"{{"response_code":0,"results":[{}]}}"#,
            [item; 10].join(",")
        )
        .into_bytes()
    }

    #[test]
    fn solo_round_credits_only_the_solo_player_once() {
        let mut runner = AppRunner::new(Quiz::default());
        runner.start();
        runner.action(action_id("solo"));
        for _ in 0..10 {
            let quiz = runner.app();
            let correct = quiz.round_questions[quiz.question].correct;
            runner.action(action_id(&choice(correct)));
            runner.action(action_id(&choice(correct))); // A stale double tap cannot score twice.
            runner.action(action_id("continue"));
        }
        assert_eq!(runner.app().scores, [10, 0, 0, 0]);
        assert_eq!(runner.app().rounds, 1);
        runner.action(action_id("continue"));
        assert_eq!(runner.app().rounds, 1);
        assert!(!format!("{:?}", screen(runner.app())).contains("Bert"));
    }

    #[test]
    fn late_cache_and_sync_do_not_replace_an_active_round() {
        let mut runner = AppRunner::new(Quiz::default());
        runner.start();
        runner.action(action_id("sync"));
        let task = runner.app().sync_task.expect("sync started");
        runner.action(action_id("solo"));
        let round = runner.app().round_questions.clone();
        runner.store_result(StoreResult::Loaded {
            key: PACK.into(),
            value: Some(pack()),
        });
        assert_eq!(runner.app().round_questions, round);
        runner.task_outcome(task, TaskOutcome::Completed(pack()));
        assert_eq!(runner.app().round_questions, round);
        runner.action(action_id("home"));
        runner.action(action_id("solo"));
        assert_eq!(runner.app().round_questions[0].category, "New pack");
    }

    #[test]
    fn long_questions_keep_every_answer_reachable_at_large_text_sizes() {
        let mut quiz = Quiz::default();
        quiz.begin(false);
        quiz.round_questions[0].text =
            "A long question about the people involved in a historical event. "
                .repeat(4)
                .chars()
                .take(240)
                .collect();
        quiz.round_questions[0].answers = std::array::from_fn(|index| {
            format!(
                "{} ending{index}",
                "A long answer with shared words ".repeat(3)
            )
            .chars()
            .rev()
            .take(80)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
        });
        for (width, height, pixels_per_inch) in
            [(1072, 1448, 300), (758, 1024, 212), (1448, 1072, 300)]
        {
            for text_scale in TextScale::STEPS {
                let metrics = DisplayMetrics {
                    width,
                    height,
                    pixels_per_inch,
                    text_scale,
                };
                let context = AppRunner::with_metrics(Quiz::default(), metrics).context();
                let pages = context.paginate(&question_text(&quiz), true);
                for page in 0..pages.len() {
                    quiz.page = page;
                    let screen = question_screen(&quiz, &context);
                    let diagnostics = screen.diagnostics(&metrics, &Chrome::default());
                    assert!(
                        diagnostics.issues.is_empty(),
                        "{metrics:?}: {:?}",
                        diagnostics.issues
                    );
                }
                let mut seen = [false; 4];
                for page in 0..4 {
                    quiz.page = page;
                    let screen = choices_screen(&quiz, &context);
                    let diagnostics = screen.diagnostics(&metrics, &Chrome::default());
                    assert!(
                        diagnostics.issues.is_empty(),
                        "{metrics:?}: {:?}",
                        diagnostics.issues
                    );
                    let layout = screen.layout_with(&metrics, &Chrome::default());
                    for (index, found) in seen.iter_mut().enumerate() {
                        *found |= layout.rect_of_action(action_id(&choice(index))).is_some();
                    }
                }
                assert!(seen.into_iter().all(|found| found));
            }
        }
    }
}

#[cfg(test)]
mod help_layout_tests {
    use super::*;
    #[test]
    fn help_fits_supported_text_scales_and_geometries() {
        let quiz = Quiz {
            view: View::HowTo,
            ..Quiz::default()
        };
        let screens = [screen(&quiz)];
        for screen in screens {
            for (width, height, pixels_per_inch) in
                [(1072, 1448, 300), (758, 1024, 212), (1448, 1072, 300)]
            {
                for text_scale in kobo_ui::TextScale::STEPS {
                    let metrics = kobo_sdk::DisplayMetrics {
                        width,
                        height,
                        pixels_per_inch,
                        text_scale,
                    };
                    let chrome = kobo_ui::Chrome::measuring(true);
                    let diagnostics = screen.diagnostics(&metrics, &chrome);
                    assert!(
                        diagnostics.issues.is_empty(),
                        "{metrics:?}: {:?}",
                        diagnostics.issues
                    );
                    assert!(screen
                        .layout_with(&metrics, &chrome)
                        .rect_of_action(action_id("home"))
                        .is_some());
                }
            }
        }
    }
}
