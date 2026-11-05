//! CSS page staging: eligibility admission and frame ownership for a styled
//! page presentation that is not wired into the shipping reader.
//!
//! Production keeps CSS selection disabled. Admission computes and records
//! eligibility when a document arrives, preparation is never invoked, and no
//! frame is ever published: the semantic reader shows every page exactly as
//! before. Tests install a deterministic preparation and enable the seam to
//! exercise the ownership lifecycle a real styled pipeline will have to
//! honour. A refused or cleared page always means the semantic reader;
//! nothing here ever changes what is on screen.
//!
//! Tests can also enable a PRODUCER mode (116, design v3): the boundary's
//! private local closure then supplies the preparation answer through
//! [`CssStage::admit_test`], invoked exactly once at Preparing, so one
//! actual arrival yields a real Stage-owned prep artifact
//! ([`crate::arrival_prep::FramePayload`]). The plumbing is generic and
//! proves no byte/Document identity; the artifact records
//! `Binding::Unestablished`. The fake seam and its tests are unchanged.
//!
//! Honesty boundaries, both deliberate. Eligibility is a PRELIMINARY gate
//! over the semantic [`Document`] conjoined with its parsed-source facts
//! (accepted parsed-facts engine): content classes the semantic model drops
//! silently -- vanished controls, media, frames, empty links and friends --
//! now refuse by name, and missing, incomplete or namespace-unsafe source
//! proof refuses `UnprovenSource` before anything else. Passing is still not
//! proof the raw source was fully rendered: whole-style-run proof (exact
//! source/sheets identity, CSS support and layout) stays an open gate, and
//! [`Admission`] carries no source or style bytes because the byte-lifetime
//! decision is still open. No complete-diagnostics claim is made.

// Production keeps the seam disabled, so the frame-carrying paths below are
// constructed only by the enabled (test and future pipeline) side. Silence
// the resulting dead-code reports in production builds, per item, nowhere
// else.
#![cfg_attr(not(test), allow(dead_code))]

use kobo_sdk::DisplayMetrics;
use kobo_web_document::source_facts::ParsedQuirks;
use kobo_web_document::Document;

use crate::forms;

/// Why a page is not, or is no longer, a CSS candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// Links, forms or other interactive content belong to the semantic
    /// reader: the single-screen stage has no hit targets.
    Interactive,
    /// Images or other media are outside the text-only stage.
    Media,
    /// The parse dropped or recovered content, so a partial restyle cannot
    /// be faithful to the page.
    ParseLoss,
    /// The reader's text scale is not the default this stage is qualified
    /// at; accessibility settings are never silently ignored.
    NonDefaultScale,
    /// Parsed-source proof is missing or unsafe: no parser-produced facts,
    /// an incomplete scan, source loss, discarded namespace evidence or a
    /// quirks-mode parse. Fail-closed: no facts means no candidacy.
    UnprovenSource,
    /// Content classes the semantic model drops silently were present in the
    /// parsed source, so a single-screen styled frame cannot be faithful.
    VanishedScript,
    VanishedForm,
    VanishedLink,
    VanishedImageMap,
    VanishedFrame,
    VanishedDisclosure,
    VanishedDialog,
    VanishedImage,
    VanishedMedia,
    VanishedForeign,
    UnknownElements,
    HeadAffectingContent,
    TemplateContent,
    InteractionAttributes,
    /// Size, orientation, scale or panel identity changed after admission,
    /// so the prepared frame no longer matches the screen.
    StaleMetrics,
    /// Injected preparation declined the page.
    Preparation,
    /// A replacement frame could not be prepared without disturbing the one
    /// on screen.
    Publication,
}

/// A prepared frame's identity, distinct for every successful preparation.
///
/// This is in-memory ownership plumbing only: it is not an SDK picture
/// handle, and no queue, commit, acknowledgement or screen binding exists
/// yet. Tests record drop order to prove in-memory state ownership --
/// replacement installed before predecessor released -- which says nothing
/// about pixel publication atomicity; that gate stays open.
#[derive(Debug)]
pub(crate) struct FrameHandle {
    id: u64,
    #[cfg(test)]
    drops: Option<std::rc::Rc<std::cell::RefCell<Vec<u64>>>>,
    /// The producer mode's real artifact (116). The fake seam leaves this
    /// `None`: its tests prove caller wiring and ownership, not pixels.
    #[cfg(test)]
    payload: Option<crate::arrival_prep::FramePayload>,
}

impl FrameHandle {
    #[cfg(test)]
    pub(crate) fn new_for_test(id: u64, drops: std::rc::Rc<std::cell::RefCell<Vec<u64>>>) -> Self {
        Self {
            id,
            drops: Some(drops),
            payload: None,
        }
    }

    /// A frame carrying the real artifact; the id is the admission's
    /// generation, distinct for every fresh arrival.
    #[cfg(test)]
    pub(crate) fn new_with_payload(
        id: u64,
        drops: std::rc::Rc<std::cell::RefCell<Vec<u64>>>,
        payload: crate::arrival_prep::FramePayload,
    ) -> Self {
        Self {
            id,
            drops: Some(drops),
            payload: Some(payload),
        }
    }

    #[cfg(test)]
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Borrows the owned artifact; the payload is never cloned out.
    #[cfg(test)]
    pub(crate) fn payload(&self) -> Option<&crate::arrival_prep::FramePayload> {
        self.payload.as_ref()
    }
}

#[cfg(test)]
impl Drop for FrameHandle {
    fn drop(&mut self) {
        if let Some(drops) = &self.drops {
            drops.borrow_mut().push(self.id);
        }
    }
}

/// The moment an admission ran: what the frame would be measured for.
/// Metrics are compared whole so any change -- size, rotation, scale --
/// retires the frame; panel identity rides alongside.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Fingerprint {
    metrics: DisplayMetrics,
    colour: bool,
}

impl Fingerprint {
    fn new(metrics: &DisplayMetrics, colour: bool) -> Self {
        Self {
            metrics: *metrics,
            colour,
        }
    }

    /// The moment's metrics, for the producer's preflight and diagnostics.
    #[cfg(test)]
    pub(crate) fn metrics(&self) -> &DisplayMetrics {
        &self.metrics
    }

    /// The moment's colour flag, for the producer's diagnostics.
    #[cfg(test)]
    pub(crate) fn colour(&self) -> bool {
        self.colour
    }
}

/// Where a document stands in the CSS staging lifecycle.
#[derive(Debug)]
pub(crate) enum Stage {
    /// No CSS state: before the first document, and after every clear.
    Disabled,
    /// The page passed the preliminary gate: the semantic checks and the
    /// parsed-source facts (no vanished controls, media, frames, empty
    /// links or unproven source). Whole-style-run proof and the byte
    /// lifetime remain open gates before any enabling.
    /// Production stops here; nothing is prepared or published.
    PreliminarySemanticCandidate,
    /// Injected preparation is working on `generation`.
    Preparing { generation: u64 },
    /// A frame for `generation` is held. Never reached in production.
    Ready {
        frame: Box<FrameHandle>,
        generation: u64,
    },
    /// The page was refused; the semantic reader carries it.
    Refused(Refusal),
}

/// The facts preparation may see: a generation and the moment it runs in.
/// Deliberately no source or style bytes -- the retention decision is open --
/// so an injected preparation cannot prove raw-source or style-run facts.
#[derive(Debug)]
pub(crate) struct Admission {
    pub(crate) generation: u64,
    #[allow(dead_code)] // Read by the real pipeline, not the test fake.
    pub(crate) fingerprint: Fingerprint,
}

#[cfg(test)]
impl Admission {
    /// A hand-built admission, for tests that drive `finish` directly.
    pub(crate) fn for_test(generation: u64, metrics: &DisplayMetrics, colour: bool) -> Self {
        Self {
            generation,
            fingerprint: Fingerprint::new(metrics, colour),
        }
    }
}

/// What injected preparation decided.
#[derive(Debug)]
pub(crate) enum PrepOutcome {
    Ready(Box<FrameHandle>),
    Refused(Refusal),
    /// Producer mode's named host refusal: the stage records the host
    /// reason in its test diagnostics and enters `Refused(reason)`.
    #[cfg(test)]
    RefusedNamed(Refusal, crate::arrival_prep::HostRefusal),
}

/// The seam a real styled pipeline will fill. Production never installs
/// one; tests install a deterministic fake.
pub(crate) trait Preparation {
    fn prepare(&mut self, admission: &Admission) -> PrepOutcome;
}

/// The production binding. Unreachable: production never enables the seam,
/// so `prepare` is never called.
struct NullPreparation;

impl Preparation for NullPreparation {
    fn prepare(&mut self, _admission: &Admission) -> PrepOutcome {
        PrepOutcome::Refused(Refusal::Preparation)
    }
}

/// The shared admission gate's outcome: stop, or drive preparation for
/// the admitted generation exactly once.
enum BeginAdmit {
    /// Refused, or a disabled seam stopped at the preliminary candidate.
    Stopped,
    /// The stage is Preparing; the caller owes exactly one `finish`.
    Preparing {
        generation: u64,
        fingerprint: Fingerprint,
    },
}

/// Admission and ownership state for at most one document's CSS frame.
pub(crate) struct CssStage {
    /// Whether injected preparation runs. False in production, so admission
    /// records eligibility and stops at [`Stage::PreliminarySemanticCandidate`].
    enabled: bool,
    /// TEST-ONLY producer mode (116): `admit_test` invokes the boundary's
    /// real producer closure instead of the injected preparation, and
    /// `reprepare` is a no-op - fresh arrivals only.
    #[cfg(test)]
    producer: bool,
    preparation: Box<dyn Preparation>,
    stage: Stage,
    /// Strictly increasing per admitted document; stale answers are dropped.
    generation: u64,
    fingerprint: Option<Fingerprint>,
    /// TEST-ONLY: the most recent named host refusal, if any. Numbers may
    /// persist in test state; pixels never do.
    #[cfg(test)]
    last_host_refusal: Option<crate::arrival_prep::HostRefusal>,
}

impl CssStage {
    /// The production state: disabled, with the unreachable binding.
    pub(crate) fn production() -> Self {
        Self {
            enabled: false,
            preparation: Box::new(NullPreparation),
            stage: Stage::Disabled,
            generation: 0,
            fingerprint: None,
            #[cfg(test)]
            producer: false,
            #[cfg(test)]
            last_host_refusal: None,
        }
    }

    /// An enabled seam driven by `preparation`, for tests.
    #[cfg(test)]
    pub(crate) fn with_preparation(preparation: Box<dyn Preparation>) -> Self {
        Self {
            enabled: true,
            preparation,
            ..Self::production()
        }
    }

    /// TEST-ONLY producer mode (116): enabled, with the boundary's real
    /// producer invoked through [`CssStage::admit_test`] instead of an
    /// injected preparation. `reprepare` is a no-op in this mode.
    #[cfg(test)]
    pub(crate) fn enable_for_test() -> Self {
        Self {
            enabled: true,
            producer: true,
            ..Self::production()
        }
    }

    #[cfg(test)]
    pub(crate) fn stage(&self) -> &Stage {
        &self.stage
    }

    /// Borrows the Ready payload, while held. The borrow excludes clear,
    /// refusal and replacement (all `&mut self`) for its lifetime.
    #[cfg(test)]
    pub(crate) fn ready_payload(&self) -> Option<&crate::arrival_prep::FramePayload> {
        match &self.stage {
            Stage::Ready { frame, .. } => frame.payload(),
            _ => None,
        }
    }

    /// The most recent named host refusal, if any.
    #[cfg(test)]
    pub(crate) fn host_refusal(&self) -> Option<crate::arrival_prep::HostRefusal> {
        self.last_host_refusal
    }

    /// Records that a document arrived. Any earlier frame is cleared first:
    /// a new document never shows another page's pixels. Preparation, when
    /// enabled, is reached exactly once per admission. In test builds the
    /// boundary uses `admit_test`; the fake-seam tests drive this directly.
    pub(crate) fn admit(&mut self, document: &Document, metrics: &DisplayMetrics, colour: bool) {
        if let BeginAdmit::Preparing {
            generation,
            fingerprint,
        } = self.begin_admit(document, metrics, colour)
        {
            let outcome = self.preparation.prepare(&Admission {
                generation,
                fingerprint,
            });
            self.finish(generation, outcome);
        }
    }

    /// The ONE common admission gate, in the same order for `admit` and
    /// `admit_test`: clear, generation, fingerprint, eligibility, the
    /// preliminary candidate, then - enabled only - Preparing. Shared so
    /// the two paths cannot drift.
    fn begin_admit(
        &mut self,
        document: &Document,
        metrics: &DisplayMetrics,
        colour: bool,
    ) -> BeginAdmit {
        self.clear();
        // Saturates rather than wraps: a reused generation would accept
        // stale answers, and no session reaches u64 admissions anyway.
        self.generation = self.generation.saturating_add(1);
        let generation = self.generation;
        let fingerprint = Fingerprint::new(metrics, colour);
        self.fingerprint = Some(fingerprint);
        if let Some(refusal) = eligibility(document, metrics) {
            self.stage = Stage::Refused(refusal);
            return BeginAdmit::Stopped;
        }
        self.stage = Stage::PreliminarySemanticCandidate;
        if !self.enabled {
            return BeginAdmit::Stopped;
        }
        self.stage = Stage::Preparing { generation };
        BeginAdmit::Preparing {
            generation,
            fingerprint,
        }
    }

    /// TEST-ONLY (116): admits through the SAME common gate, with the
    /// preparation answer supplied by the boundary's producer closure.
    /// This is crate-private generic pairing plumbing: it cannot prove the
    /// closure's bytes are the arriving document's bytes - only the
    /// reviewed `show_document_noted` callsite associates live bytes, and
    /// the artifact records `Binding::Unestablished`. Disabled stops at
    /// the preliminary candidate, production-identical, with the closure
    /// dropped uninvoked. Fake mode (`with_preparation`) invokes the
    /// injected preparation exactly as `admit` does, the closure dropped
    /// uninvoked. Producer mode (`enable_for_test`) invokes the closure
    /// EXACTLY ONCE at `Preparing`, then finishes.
    #[cfg(test)]
    pub(crate) fn admit_test(
        &mut self,
        document: &Document,
        metrics: &DisplayMetrics,
        colour: bool,
        produce: impl FnOnce(&Admission) -> PrepOutcome,
    ) {
        let BeginAdmit::Preparing {
            generation,
            fingerprint,
        } = self.begin_admit(document, metrics, colour)
        else {
            return;
        };
        let admission = Admission {
            generation,
            fingerprint,
        };
        let outcome = if self.producer {
            produce(&admission)
        } else {
            drop(produce);
            self.preparation.prepare(&admission)
        };
        self.finish(generation, outcome);
    }

    /// Receives a preparation answer. An answer for any generation but the
    /// current one is stale: its document is gone and the answer is dropped.
    /// All preparation is synchronous today, so this guard is defensive; an
    /// async pipeline needs an explicit freshness contract (attempt token,
    /// document, metrics and face generation) before activation.
    pub(crate) fn finish(&mut self, generation: u64, outcome: PrepOutcome) {
        if !matches!(self.stage, Stage::Preparing { generation: current } if current == generation)
        {
            return;
        }
        self.stage = match outcome {
            PrepOutcome::Ready(frame) => Stage::Ready { frame, generation },
            PrepOutcome::Refused(reason) => Stage::Refused(reason),
            #[cfg(test)]
            PrepOutcome::RefusedNamed(reason, host) => {
                self.last_host_refusal = Some(host);
                Stage::Refused(reason)
            }
        };
    }

    /// Prepares a replacement for the frame being held, keeping it published
    /// until the replacement exists: the new frame is installed before the
    /// old one is dropped. A failed replacement takes the whole frame down
    /// rather than leaving pixels measured for a moment that has passed.
    ///
    /// Only reachable when the seam is enabled; production never holds a
    /// frame. The trigger for this (a resize path that keeps the document)
    /// does not exist yet; tests drive it directly. It reuses the
    /// admission's generation; a real asynchronous reprepare needs its own
    /// attempt token plus document, metrics and face-generation checks.
    #[cfg(test)]
    pub(crate) fn reprepare(&mut self) {
        if self.producer {
            // Producer mode prepares fresh arrivals only: never the
            // NullPreparation, never bytes measured for a passed moment.
            return;
        }
        let Some(fingerprint) = self.fingerprint else {
            return;
        };
        if !self.enabled || !matches!(self.stage, Stage::Ready { .. }) {
            return;
        }
        let generation = self.generation;
        // The old frame stays owned by `previous` for the whole preparation:
        // this is the one moment two frames exist, and the published one is
        // released only after its replacement is installed.
        let previous = std::mem::replace(&mut self.stage, Stage::Preparing { generation });
        let outcome = self.preparation.prepare(&Admission {
            generation,
            fingerprint,
        });
        self.stage = match outcome {
            PrepOutcome::Ready(frame) => Stage::Ready { frame, generation },
            PrepOutcome::Refused(reason) => Stage::Refused(reason),
            PrepOutcome::RefusedNamed(reason, host) => {
                self.last_host_refusal = Some(host);
                Stage::Refused(reason)
            }
        };
        drop(previous);
    }

    /// Drops any prepared or preparing state. Idempotent.
    pub(crate) fn clear(&mut self) {
        self.stage = Stage::Disabled;
        self.fingerprint = None;
        #[cfg(test)]
        {
            self.last_host_refusal = None;
        }
    }

    /// Forgets the frame when the moment it was measured for has passed:
    /// a resize, rotation, scale or panel change since admission.
    pub(crate) fn validate(&mut self, metrics: &DisplayMetrics, colour: bool) {
        let Some(fingerprint) = self.fingerprint else {
            return;
        };
        if fingerprint != Fingerprint::new(metrics, colour) {
            self.stage = Stage::Refused(Refusal::StaleMetrics);
        }
    }
}

/// Whether `document` can be a CSS candidate at all. This is the
/// document-level gate only: it judges the semantic document conjoined
/// with its parsed-source facts, not the raw
/// source, and layout-level refusals (overflow, fractional metrics,
/// unsupported or missing sheets) belong to a preparation that actually
/// sees the style run--which the injected seam does not.
fn eligibility(document: &Document, metrics: &DisplayMetrics) -> Option<Refusal> {
    // Parsed-source proof first: without it nothing else is known to be the
    // whole page. Fail-closed: a Document the parser did not produce carries
    // no facts and cannot be a candidate.
    let Some(facts) = document.source_facts.as_ref() else {
        return Some(Refusal::UnprovenSource);
    };
    if !facts.scan_completed
        || !facts.source_loss_free
        || facts.attr_namespace_discarded
        || facts.quirks != ParsedQuirks::NoQuirks
    {
        return Some(Refusal::UnprovenSource);
    }
    // What the semantic reader shows, named as before.
    if !document.links.is_empty() || !forms::forms(&document.blocks).is_empty() {
        return Some(Refusal::Interactive);
    }
    if !document.images().is_empty() {
        return Some(Refusal::Media);
    }
    if !document.warnings.is_empty() || document.parse_errors > 0 {
        return Some(Refusal::ParseLoss);
    }
    if metrics.text_scale != DisplayMetrics::default().text_scale {
        return Some(Refusal::NonDefaultScale);
    }
    // What vanished from the semantic model, per the parsed-source facts.
    let profile = &facts.profile;
    if profile.scripts > 0 {
        return Some(Refusal::VanishedScript);
    }
    if profile.forms > 0 {
        return Some(Refusal::VanishedForm);
    }
    if profile.links > 0 {
        return Some(Refusal::VanishedLink);
    }
    if profile.image_maps > 0 {
        return Some(Refusal::VanishedImageMap);
    }
    if profile.frames > 0 {
        return Some(Refusal::VanishedFrame);
    }
    if profile.details_summary > 0 {
        return Some(Refusal::VanishedDisclosure);
    }
    if profile.dialogs > 0 {
        return Some(Refusal::VanishedDialog);
    }
    if profile.images > 0 {
        return Some(Refusal::VanishedImage);
    }
    if profile.media > 0 {
        return Some(Refusal::VanishedMedia);
    }
    if profile.svg_math > 0 || profile.foreign_other > 0 {
        return Some(Refusal::VanishedForeign);
    }
    if profile.unknown_custom > 0 {
        return Some(Refusal::UnknownElements);
    }
    if profile.head_affecting > 0 {
        return Some(Refusal::HeadAffectingContent);
    }
    if profile.template > 0 {
        return Some(Refusal::TemplateContent);
    }
    if profile.interaction_attrs > 0 {
        return Some(Refusal::InteractionAttributes);
    }
    None
}
