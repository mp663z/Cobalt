//! 116 producer mode (design v3): ONE actual arrival through the app's
//! real boundary yields a REAL preparation artifact OWNED BY THE STAGE -
//! the `DisplayList`, the RGBA raster, FIXED diagnostics and a session
//! SNAPSHOT - via the real style run, the real engine paint path and the
//! accepted 115 native font session. Everything here is cfg(test); the
//! frame is never displayed, and production is byte-for-byte unchanged.
//!
//! Honest seam: this producer is generic pairing plumbing. It cannot
//! prove the bytes it paints are the bytes the semantic reader parsed;
//! only the reviewed `show_document_noted` callsite associates the live
//! arriving bytes. Diagnostics therefore record [`Binding::Unestablished`].
//! This is not B0, production activation, device/sim parity, a
//! source-identity guarantee or resource/budget adoption.

#![cfg(test)]

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use kobo_sdk::DisplayMetrics;
use kobo_text::diagnostic_font_session::{NativeFontSnapshot, NativeGlyphError};
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_text_page::{paint_single_text_page, PageError};
use kobo_web_document::display_list::{DisplayList, Error as DisplayError, Rgb, MAX_PIXELS};
use kobo_web_document::style_diagnostics::StyleDiagnostics;
use kobo_web_document::{parse_style_tree, Limits};

use crate::css_stage::{Admission, FrameHandle, PrepOutcome, Refusal};
use crate::native_font_adapter::NativeFontAdapter;

/// The explicit payload ceiling: RGBA bytes for the largest paintable
/// viewport. Preflighted together with the pixel bound BEFORE any font
/// construction or raster; the existing mask, call and command caps are
/// unchanged.
pub(crate) const MAX_PAYLOAD_RGBA_BYTES: usize = MAX_PIXELS * 4;

/// Renderer identity is not established: the plumbing is generic, so the
/// artifact is recorded as unbound rather than claimed to be the arriving
/// document's own rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Binding {
    Unestablished,
}

/// A named host refusal, recorded in the stage's test diagnostics. Every
/// refusal releases partial locals and yields NO payload, NO Ready and NO
/// partial publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostRefusal {
    /// Preflight: negative, zero or over-cap viewport dimensions. This is
    /// the painter's `InvalidViewport` bound (`css_text_page.rs` checks it
    /// before any provider/font work); preflight enforces it before font
    /// construction even begins.
    InvalidViewport,
    /// Preflight: the RGBA payload would exceed the explicit ceiling.
    PayloadCeiling,
    /// The native font session could not be constructed at all.
    FontConstruction,
    /// The real engine paint path refused the page whole.
    Paint(PageError),
    /// The display list refused to rasterize.
    Raster(DisplayError),
    /// The session carried a sticky error despite a successful paint.
    Font(NativeGlyphError),
}

/// The FIXED diagnostics recorded at preparation. They may persist in
/// test state after the payload is released; pixels may not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameDiagnostics {
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) commands: usize,
    pub(crate) glyph_bytes: usize,
    /// Dark pixels in the raster: real glyph ink, counted like the 115 proof.
    pub(crate) ink: usize,
    /// The style run's own FIXED diagnostics snapshot, retained whole:
    /// the existing 23-reason `StyleDiagnostics`, not a reduction.
    pub(crate) style: StyleDiagnostics,
    pub(crate) generation: u64,
    pub(crate) fingerprint_metrics: DisplayMetrics,
    pub(crate) fingerprint_colour: bool,
    pub(crate) binding: Binding,
}

/// The stage-owned real preparation artifact. The stage is its sole
/// owner, via the Ready [`FrameHandle`]: it is not `Clone`, pixels and the
/// `DisplayList` are never cloned elsewhere (accessors return borrows
/// only), and because `ready_payload()` borrows the stage, no clear,
/// refusal or replacement (all `&mut self`) can run while an inspection
/// borrow is live - the borrow checker enforces no-live-borrow-at-clear.
/// The drop witness lives inside the payload: its `Weak` corroborates
/// each release (clear, refusal, fresh-arrival replacement, stale-outcome
/// drop); the structure is the proof.
pub(crate) struct FramePayload {
    list: DisplayList,
    rgba: Vec<u8>,
    diagnostics: FrameDiagnostics,
    /// FIXED snapshot taken after the paint. The session, the face and the
    /// adapter were dropped before Ready: no cache exists, fresh arrivals
    /// only, construction memory is unmeasured - nothing font-side
    /// outlives preparation.
    snapshot: NativeFontSnapshot,
    witness: Rc<()>,
}

impl std::fmt::Debug for FramePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FramePayload")
            .field("diagnostics", &self.diagnostics)
            .field("snapshot", &self.snapshot)
            .finish_non_exhaustive()
    }
}

impl FramePayload {
    pub(crate) fn list(&self) -> &DisplayList {
        &self.list
    }
    pub(crate) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    pub(crate) fn diagnostics(&self) -> &FrameDiagnostics {
        &self.diagnostics
    }
    pub(crate) fn snapshot(&self) -> &NativeFontSnapshot {
        &self.snapshot
    }
    /// A witnessing handle: alive exactly while the payload is owned.
    pub(crate) fn witness(&self) -> Weak<()> {
        Rc::downgrade(&self.witness)
    }
}

fn refuse(host: HostRefusal) -> PrepOutcome {
    PrepOutcome::RefusedNamed(Refusal::Preparation, host)
}

/// The real producer, invoked exactly once per admission at `Preparing`
/// by the boundary's private local closure. Preflight bounds run BEFORE
/// any font construction or raster; then the real style run (no sheets:
/// engine defaults apply - `ua_display`, body/p margins, ua bold/italic
/// defaults; that is not font synthesis), the real engine paint path and
/// the raster at the fingerprint's dims. Any exceed or failure refuses
/// whole. `drops` is the test rig's drop-order log.
pub(crate) fn prepare_real_frame(
    bytes: &[u8],
    admission: &Admission,
    drops: &Rc<RefCell<Vec<u64>>>,
) -> PrepOutcome {
    let metrics = admission.fingerprint.metrics();
    // PREFLIGHT, before any font construction or raster: negative, zero or
    // over-cap dimensions refuse with the painter's InvalidViewport bound
    // named. The painter re-checks the same bound before fonts.
    let (Ok(width), Ok(height)) = (u32::try_from(metrics.width), u32::try_from(metrics.height))
    else {
        return refuse(HostRefusal::InvalidViewport);
    };
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)));
    if width == 0 || height == 0 || pixels.is_none_or(|n| n > MAX_PIXELS) {
        return refuse(HostRefusal::InvalidViewport);
    }
    let pixels = pixels.expect("nonzero in-cap dimensions");
    let Some(rgba_len) = pixels.checked_mul(4) else {
        return refuse(HostRefusal::PayloadCeiling);
    };
    if rgba_len > MAX_PAYLOAD_RGBA_BYTES {
        return refuse(HostRefusal::PayloadCeiling);
    }
    // The real style run with NO sheets (the default/UA-sheet decision
    // stays open): engine defaults only.
    let styled = parse_style_tree(bytes, &[], &Limits::DEFAULT);
    let style_diagnostics = styled.diagnostics.clone();
    let tree = BoxTree::from_style(&styled);
    // Font construction happens only after every preflight bound passed.
    let Some(adapter) = NativeFontAdapter::new() else {
        return refuse(HostRefusal::FontConstruction);
    };
    let list = match paint_single_text_page(&tree, width, height, &adapter) {
        Ok(list) => list,
        Err(error) => return refuse(HostRefusal::Paint(error)),
    };
    let rgba = match list.rasterize(width, height, Rgb(255, 255, 255)) {
        Ok(rgba) => rgba,
        Err(error) => return refuse(HostRefusal::Raster(error)),
    };
    debug_assert_eq!(rgba.len(), rgba_len, "raster size matches preflight");
    // The FIXED snapshot; then the session, face and adapter are dropped
    // BEFORE Ready so nothing font-side outlives preparation.
    let snapshot = adapter.snapshot();
    drop(adapter);
    if let Some(error) = snapshot.first_error {
        return refuse(HostRefusal::Font(error));
    }
    let ink = rgba
        .chunks_exact(4)
        .filter(|px| px[0] < 128 && px[1] < 128 && px[2] < 128)
        .count();
    let diagnostics = FrameDiagnostics {
        viewport_width: width,
        viewport_height: height,
        commands: list.commands().len(),
        glyph_bytes: list.glyph_bytes(),
        ink,
        style: style_diagnostics,
        generation: admission.generation,
        fingerprint_metrics: *metrics,
        fingerprint_colour: admission.fingerprint.colour(),
        binding: Binding::Unestablished,
    };
    let payload = FramePayload {
        list,
        rgba,
        diagnostics,
        snapshot,
        witness: Rc::new(()),
    };
    PrepOutcome::Ready(Box::new(FrameHandle::new_with_payload(
        admission.generation,
        Rc::clone(drops),
        payload,
    )))
}
