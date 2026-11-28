//! Host supply of external `img` bodies for Page view (PNG and JPEG only).
//!
//! The page paints first with alt text; this module then fetches the document's external images through the same
//! net layer as stylesheets, and hands the bodies back keyed by the `src` as authored. Everything here is a refusal
//! of one image, never of the page: an image that fails for any reason keeps its alt text.
//!
//! Limits, all enforced here except where noted:
//! - at most [`MAX_IMAGES`] distinct sources per page (the rest are never requested);
//! - at most [`MAX_IMAGE_BYTES`] per body (passed to the fetch, which fails an oversize body) and
//!   [`MAX_TOTAL_BYTES`] across the page (a body that would pass the total is dropped);
//! - a body is kept only when it starts with a PNG or JPEG signature: the `Content-Type` header is not trusted and
//!   is not even visible here, so SVG, GIF, WebP, HTML error pages and renamed files are refused by sniffing;
//! - `https` only, no credentials in the URL, no `Authorization`: the request carries `credential: None` and an
//!   `Accept` header only. The net layer owns redirects (at most five, never a downgrade to http, same-origin header
//!   forwarding) and its connect, header and body timeouts; it keeps no cookie jar, so no cookie is sent to any
//!   third party. Redirect count/TLS upgrade and stalled cancellation are covered by `kobo-net/tests/image_transport_limits.rs`; the idle timeout boundary has a unit test.
//! Pixel budget (1 Mpx across the page) and decode refusal are applied later by `page_images`.

use std::collections::{BTreeMap, VecDeque};

use kobo_sdk::{Context, Header, Task, TaskId, TaskOutcome};

use crate::page_images::External;

pub const MAX_IMAGES: usize = 16;
pub const MAX_IMAGE_BYTES: usize = 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
pub const ACCEPT: &str = "image/png,image/jpeg;q=0.9";

/// Why one image was left out. Counted, never shown as an error.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    pub kept: usize,
    /// Over the per-page source cap, so never requested.
    pub not_requested: usize,
    /// The fetch failed (network, status, oversize body, plain-http redirect, cancel).
    pub failed: usize,
    /// Body is not a PNG or JPEG by signature.
    pub wrong_type: usize,
    /// Body would pass the page's total byte budget.
    pub over_budget: usize,
    /// No runtime slot for another fetch.
    pub no_slot: usize,
}

#[derive(Clone, Copy, Debug)]
struct Limits {
    images: usize,
    image_bytes: usize,
    total_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            images: MAX_IMAGES,
            image_bytes: MAX_IMAGE_BYTES,
            total_bytes: MAX_TOTAL_BYTES,
        }
    }
}

#[derive(Debug, Default)]
pub struct ImageSupply {
    limits: Limits,
    pending: VecDeque<(String, String)>,
    by_task: BTreeMap<TaskId, String>,
    bodies: External,
    received: usize,
    counts: Counts,
    settled: bool,
}

/// PNG or JPEG by the first bytes. Nothing else is ever decoded.
#[must_use]
pub fn sniff(body: &[u8]) -> bool {
    body.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
        || body.starts_with(&[0xff, 0xd8, 0xff])
}

impl ImageSupply {
    /// Cancels outstanding fetches and forgets everything. Safe when idle.
    pub fn clear(&mut self, context: &mut Context) {
        for (task, _) in std::mem::take(&mut self.by_task) {
            context.cancel(task);
        }
        *self = Self {
            limits: self.limits,
            ..Self::default()
        };
    }

    /// Smaller limits, for tests that must reach a budget the runtime's four-task ceiling would otherwise hide.
    #[cfg(test)]
    #[must_use]
    pub fn with_limits(images: usize, image_bytes: usize, total_bytes: usize) -> Self {
        Self {
            limits: Limits {
                images,
                image_bytes,
                total_bytes,
            },
            ..Self::default()
        }
    }

    /// `Some` once every requested image has been kept or refused.
    #[must_use]
    pub fn outcome(&self) -> Option<&External> {
        self.settled.then_some(&self.bodies)
    }

    #[must_use]
    #[cfg(test)]
    pub fn counts(&self) -> Counts {
        self.counts
    }

    #[cfg(test)]
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.by_task.len()
    }

    /// Starts supply for one document from `(authored src, address)` pairs. Addresses that are not `https` are
    /// dropped by the caller's census; this re-checks and never requests them.
    pub fn begin(&mut self, context: &mut Context, sources: &[(String, kobo_web_document::Url)]) {
        self.clear(context);
        let mut requested = 0;
        for (src, url) in sources {
            if url.scheme() != kobo_web_document::Scheme::Https {
                self.counts.failed += 1;
                continue;
            }
            if requested >= self.limits.images {
                self.counts.not_requested += 1;
                continue;
            }
            requested += 1;
            self.pending
                .push_back((src.clone(), url.without_fragment().to_string()));
        }
        self.pump(context);
        self.settle_if_done();
    }

    /// Queue beyond the runtime's shared task ceiling instead of dropping valid images.
    pub fn pump(&mut self, context: &mut Context) {
        if self.settled {
            return;
        }
        while context.tasks_in_flight() < kobo_sdk::MAX_TASKS_IN_FLIGHT {
            let Some((src, url)) = self.pending.pop_front() else {
                break;
            };
            let work = Task::Fetch {
                url,
                offset: 0,
                max_bytes: u32::try_from(self.limits.image_bytes).unwrap_or(u32::MAX),
                credential: None,
                headers: vec![Header::new("Accept", ACCEPT)],
            };
            match context.spawn(work) {
                Some(task) => {
                    self.by_task.insert(task, src);
                }
                None => {
                    self.counts.no_slot += 1 + self.pending.len();
                    self.pending.clear();
                }
            }
        }
        self.settle_if_done();
    }

    /// True when `task` was one of ours (and is consumed).
    pub fn on_task(&mut self, context: &mut Context, task: TaskId, outcome: &TaskOutcome) -> bool {
        let Some(src) = self.by_task.remove(&task) else {
            return false;
        };
        match outcome {
            TaskOutcome::Completed(body) => {
                if body.len() > self.limits.image_bytes {
                    self.counts.failed += 1;
                } else if !sniff(body) {
                    self.counts.wrong_type += 1;
                } else if self.received.saturating_add(body.len()) > self.limits.total_bytes {
                    self.counts.over_budget += 1;
                } else {
                    self.received += body.len();
                    self.counts.kept += 1;
                    self.bodies.insert(src, body.clone());
                }
            }
            TaskOutcome::Failed(_) | TaskOutcome::Cancelled => self.counts.failed += 1,
        }
        self.pump(context);
        true
    }

    fn settle_if_done(&mut self) {
        if self.pending.is_empty() && self.by_task.is_empty() {
            self.settled = true;
        }
    }
}
