//! Host supply of linked stylesheets for the (production-disabled) CSS stage.
//!
//! The document parse indexes `sheets` by the external `<link rel=stylesheet>` links it walks; the host must
//! supply exactly that many, in that order. This module owns the fetch half: it asks the library for the census
//! ([`kobo_web_document::host_stylesheet_links`]), spawns one bounded `Task::Fetch` per link, gathers the bodies in
//! link order, and settles to [`Outcome::Ready`] or [`Outcome::Failed`]. It never parses, admits or paints, and it
//! does not make CSS preparation asynchronous: that needs a freshness contract that does not exist yet.
//!
//! Bounds are the existing ones, not new ones: the link ceiling and per-sheet bytes are
//! `SupplyBudget::CANONICAL`; the fetch is https-only with at most five redirects and no truncation (a body past
//! `max_bytes` is a `TooLarge` failure) because that is the net layer's contract; no credential is sent. A page
//! that the parse is certain to refuse (too many links, unknown media, an unfetchable link, an incomplete census)
//! is settled `Failed` BEFORE any network traffic.
//!
//! Known conservative gaps, documented not hidden: (1) plain-http links are admitted by the library census but
//! settled `Unfetchable` here because the net layer is https-only (fail closed, no upgrade, no net-policy change);
//! (2) a body that is not UTF-8 is fetched and then refused by the parse, so supply can be `Ready` for a page the
//! parse still refuses: `Ready` means "bytes delivered in order", not "parse admitted"; (3) the media width the
//! census evaluates must equal the eventual paint width, and the caller passes the panel width today (open); (4)
//! redirect limits come from the net layer and are source-verified, not tested at this seam.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;

use kobo_sdk::{Context, Header, Task, TaskError, TaskId, TaskOutcome};
use kobo_web_document::style_tree::HostSheetLinks;
use kobo_web_document::SupplyBudget;

pub const ACCEPT: &str = "text/css,*/*;q=0.1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Why {
    /// More linked sheets than the canonical ceiling allows (data: sheets count against it).
    TooManyLinks,
    /// A link media query the parse refuses.
    UnknownMedia,
    /// A link with no usable address, or a plain-http one (the net layer is https-only).
    Unfetchable,
    /// The census was cut short (input, nodes or attributes), so the sheet list is not an inventory.
    IncompleteCensus,
    /// The runtime refused to start a fetch (task limit).
    NoSlot,
    /// A fetch failed; the net layer's reason.
    Fetch(TaskError),
    /// A fetch was cancelled by someone other than `clear`.
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Sheet bodies in the order the parse indexes them. May be empty (no external sheets).
    Ready(Vec<Vec<u8>>),
    Failed(Why),
}

#[derive(Debug)]
enum Slot {
    Waiting,
    Got(Vec<u8>),
}

#[derive(Debug, Default)]
pub struct SheetSupply {
    slots: Vec<Slot>,
    by_task: BTreeMap<TaskId, usize>,
    settled: Option<Outcome>,
}

impl SheetSupply {
    /// Cancels every outstanding fetch and forgets the result. Safe when idle.
    pub fn clear(&mut self, context: &mut Context) {
        for (task, _) in std::mem::take(&mut self.by_task) {
            context.cancel(task);
        }
        self.slots.clear();
        self.settled = None;
    }

    #[must_use]
    pub fn outcome(&self) -> Option<&Outcome> {
        self.settled.as_ref()
    }

    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.by_task.len()
    }

    /// Starts supply for one document. Replaces any earlier supply.
    pub fn begin(&mut self, context: &mut Context, census: &HostSheetLinks) {
        self.clear(context);
        let budget = SupplyBudget::CANONICAL;
        let refusal = if !census.complete {
            Some(Why::IncompleteCensus)
        } else if census.unknown_media > 0 {
            Some(Why::UnknownMedia)
        } else if census.links.len().saturating_add(census.data_links) > budget.max_linked_sheets {
            Some(Why::TooManyLinks)
        } else if census.links.iter().any(|link| {
            // The net layer is https-only and refuses plain http before connecting, so a census-admitted http
            // link is settled here instead of costing a task that cannot succeed. Fail closed, no upgrade.
            link.as_ref()
                .is_none_or(|url| url.scheme() != kobo_web_document::Scheme::Https)
        }) {
            Some(Why::Unfetchable)
        } else {
            None
        };
        if let Some(why) = refusal {
            self.settled = Some(Outcome::Failed(why));
            return;
        }
        if census.links.is_empty() {
            self.settled = Some(Outcome::Ready(Vec::new()));
            return;
        }
        let max_bytes = u32::try_from(budget.max_sheet_bytes).unwrap_or(u32::MAX);
        for (index, link) in census.links.iter().enumerate() {
            let url = link
                .as_ref()
                .expect("checked above")
                .without_fragment()
                .to_string();
            let task = context.spawn(Task::Fetch {
                url,
                offset: 0,
                max_bytes,
                credential: None,
                headers: vec![Header::new("Accept", ACCEPT)],
            });
            let Some(task) = task else {
                self.clear(context);
                self.settled = Some(Outcome::Failed(Why::NoSlot));
                return;
            };
            self.by_task.insert(task, index);
            self.slots.push(Slot::Waiting);
        }
    }

    /// True when `task` belonged to this supply (and was consumed). Other tasks are left to their owners.
    pub fn on_task(&mut self, context: &mut Context, task: TaskId, outcome: &TaskOutcome) -> bool {
        let Some(index) = self.by_task.remove(&task) else {
            return false;
        };
        match outcome {
            TaskOutcome::Completed(body) => {
                self.slots[index] = Slot::Got(body.clone());
                if self.by_task.is_empty() && self.settled.is_none() {
                    let bodies = std::mem::take(&mut self.slots)
                        .into_iter()
                        .map(|slot| match slot {
                            Slot::Got(body) => body,
                            Slot::Waiting => Vec::new(),
                        })
                        .collect();
                    self.settled = Some(Outcome::Ready(bodies));
                }
            }
            TaskOutcome::Failed(error) => self.fail(context, Why::Fetch(*error)),
            TaskOutcome::Cancelled => self.fail(context, Why::Cancelled),
        }
        true
    }

    fn fail(&mut self, context: &mut Context, why: Why) {
        if self.settled.is_some() {
            return;
        }
        for (task, _) in std::mem::take(&mut self.by_task) {
            context.cancel(task);
        }
        self.slots.clear();
        self.settled = Some(Outcome::Failed(why));
    }
}
