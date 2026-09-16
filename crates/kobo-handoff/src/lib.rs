//! The ownership transition state machine behind every resource handoff.
//!
//! Ownership transfer is measured, not assumed
//! (docs/quality/contracts/device-resource-ownership.md): a transition is
//! recorded only with the observation that proved it, the machine refuses to
//! acquire a resource whose probe shows more than one owner, an interrupted
//! hand-back rolls back and keeps its diagnostics, and a resume is proven by
//! the profile's resume evidence, never by the steps having run.
//!
//! The machine is pure: the [`Driver`] supplies probing, steps and rollback,
//! so tests script the hardware and the device binary supplies the real one.
//! Wall-clock enforcement of each step's timeout lives in the driver; the
//! machine enforces order, evidence and the single-owner invariant.

use kobo_profile::ownership::{Evidence, ResourceOwnership};

pub use kobo_profile::ownership::Driver;
use std::fmt;

/// Who currently owns the resource, as far as the machine has proven.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// The stock software owns it (probed, single owner).
    StockOwned,
    /// Cobalt holds it for the session.
    Held,
    /// Hand-back is running; the index is the step under way.
    HandingBack(usize),
    /// The stock software is proven resumed by the profile's evidence.
    Resumed,
    /// A step failed; rollback has run and diagnostics were captured.
    RolledBack,
}

/// One journaled transition: from, to, and the observation that proved it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transition {
    pub from: Phase,
    pub to: Phase,
    pub observation: String,
}

/// Why the machine refused or failed a transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandoffError {
    /// The record is unverified on this device: no handoff may be driven.
    UnverifiedRecord,
    /// The probe found owners other than the expected stock owner. Two
    /// owners of one resource is how a session leaves Wi-Fi dead until a
    /// reboot, so acquisition refuses outright.
    DoubleOwner { found: Vec<String> },
    /// The probe found no stock owner to take over from.
    NoStockOwner,
    /// A hand-back step failed; rollback ran and diagnostics were captured.
    StepFailed {
        step: usize,
        action: &'static str,
        error: String,
        diagnostics: String,
    },
    /// The steps ran but the resume observation did not prove resumption.
    ResumeUnproven { observation: String },
}

impl fmt::Display for HandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnverifiedRecord => {
                write!(
                    formatter,
                    "the ownership record is unverified on this device"
                )
            }
            Self::DoubleOwner { found } => {
                write!(
                    formatter,
                    "refusing acquisition: owners present: {}",
                    found.join(", ")
                )
            }
            Self::NoStockOwner => write!(formatter, "no stock owner found to take over from"),
            Self::StepFailed {
                step,
                action,
                error,
                ..
            } => write!(
                formatter,
                "hand-back step {step} ({action}) failed: {error}"
            ),
            Self::ResumeUnproven { observation } => {
                write!(formatter, "resume not proven: {observation}")
            }
        }
    }
}

impl std::error::Error for HandoffError {}

/// A handoff of one resource under one profile record.
pub struct Handoff<'a> {
    record: &'a ResourceOwnership,
    phase: Phase,
    journal: Vec<Transition>,
}

impl<'a> Handoff<'a> {
    /// Start a handoff from a profile record. Unverified records carry no
    /// measured claims, so nothing may be driven from them.
    ///
    /// # Errors
    ///
    /// Returns [`HandoffError::UnverifiedRecord`] for an unverified record.
    pub fn new(record: &'a ResourceOwnership) -> Result<Self, HandoffError> {
        if record.evidence != Evidence::Measured {
            return Err(HandoffError::UnverifiedRecord);
        }
        Ok(Self {
            record,
            phase: Phase::StockOwned,
            journal: Vec::new(),
        })
    }

    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The proof so far: every transition with its observation.
    #[must_use]
    pub fn journal(&self) -> &[Transition] {
        &self.journal
    }

    fn record_transition(&mut self, to: Phase, observation: String) {
        self.journal.push(Transition {
            from: self.phase,
            to,
            observation,
        });
        self.phase = to;
    }

    /// Take the resource from its stock owner. Refuses when the probe shows
    /// anything but exactly the one expected owner: a second owner left over
    /// from a previous session is the double-owner failure this exists to
    /// catch before it touches hardware.
    ///
    /// # Errors
    ///
    /// Returns [`HandoffError::DoubleOwner`] or
    /// [`HandoffError::NoStockOwner`] when the probe disagrees with the record.
    pub fn acquire(&mut self, driver: &mut dyn Driver) -> Result<(), HandoffError> {
        debug_assert_eq!(self.phase, Phase::StockOwned);
        let found = driver
            .probe_owners()
            .map_err(|error| HandoffError::StepFailed {
                step: 0,
                action: "probe owners",
                error,
                diagnostics: String::new(),
            })?;
        if found.len() == 1 && self.record.owns(&found[0]) {
            let observation = format!("probed: sole owner is {}", found[0]);
            self.record_transition(Phase::Held, observation);
            return Ok(());
        }
        if found.is_empty() {
            return Err(HandoffError::NoStockOwner);
        }
        Err(HandoffError::DoubleOwner { found })
    }

    /// Hand the resource back: each bounded step in order, then the resume
    /// observation. A failed step rolls back and keeps its diagnostics; the
    /// machine ends in [`Phase::RolledBack`], never in a claimed resume.
    ///
    /// # Errors
    ///
    /// Returns [`HandoffError::StepFailed`] after rollback, or
    /// [`HandoffError::ResumeUnproven`] when the evidence does not match.
    pub fn hand_back(&mut self, driver: &mut dyn Driver) -> Result<(), HandoffError> {
        debug_assert_eq!(self.phase, Phase::Held);
        for (index, step) in self.record.hand_back.iter().enumerate() {
            self.record_transition(Phase::HandingBack(index), format!("begin: {}", step.action));
            if let Err(error) = driver.run_step(step.action, step.timeout_seconds) {
                let diagnostics = driver.rollback().unwrap_or_else(|rollback_error| {
                    format!("rollback itself failed: {rollback_error}")
                });
                self.record_transition(Phase::RolledBack, diagnostics.clone());
                return Err(HandoffError::StepFailed {
                    step: index,
                    action: step.action,
                    error,
                    diagnostics,
                });
            }
        }
        match driver.observe_resume() {
            Ok(observation) if observation == self.record.resume_evidence => {
                self.record_transition(Phase::Resumed, observation);
                Ok(())
            }
            Ok(observation) => Err(HandoffError::ResumeUnproven { observation }),
            Err(error) => Err(HandoffError::ResumeUnproven { observation: error }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobo_profile::ownership::{self, ResourceKind};

    /// A scripted driver: each call consumes the next scripted answer.
    struct Scripted {
        owners: Vec<Vec<String>>,
        step_failures: Vec<Option<String>>,
        resume_observation: Result<String, String>,
        rolled_back: bool,
    }

    impl Scripted {
        fn clean(record: &'static ResourceOwnership) -> Self {
            Self {
                owners: vec![vec![record.owner_before_entry.to_owned()]],
                step_failures: vec![None; record.hand_back.len()],
                resume_observation: Ok(record.resume_evidence.to_owned()),
                rolled_back: false,
            }
        }
    }

    impl Driver for Scripted {
        fn probe_owners(&mut self) -> Result<Vec<String>, String> {
            Ok(self.owners.remove(0))
        }

        fn run_step(&mut self, action: &'static str, _timeout: u32) -> Result<String, String> {
            match self.step_failures.remove(0) {
                Some(error) => Err(error),
                None => Ok(format!("done: {action}")),
            }
        }

        fn observe_resume(&mut self) -> Result<String, String> {
            self.resume_observation.clone()
        }

        fn rollback(&mut self) -> Result<String, String> {
            self.rolled_back = true;
            Ok("watchdog restarted the reader; journal under /tmp".to_owned())
        }
    }

    static PANEL: ResourceOwnership =
        ownership::panel_via_reader_restart(&["/bin/wpa_supplicant"], Evidence::Measured);

    #[test]
    fn round_trip_records_every_measured_transition() {
        let mut handoff = Handoff::new(&PANEL).unwrap();
        let mut driver = Scripted::clean(&PANEL);
        handoff.acquire(&mut driver).unwrap();
        assert_eq!(handoff.phase(), Phase::Held);
        handoff.hand_back(&mut driver).unwrap();
        assert_eq!(handoff.phase(), Phase::Resumed);
        // Held, three steps, resumed: every one carries its observation.
        assert_eq!(handoff.journal().len(), 1 + 3 + 1);
        assert!(handoff
            .journal()
            .iter()
            .all(|transition| !transition.observation.is_empty()));
    }

    #[test]
    fn acquisition_refuses_when_a_second_owner_is_present() {
        let mut handoff = Handoff::new(&PANEL).unwrap();
        let mut driver = Scripted::clean(&PANEL);
        driver.owners = vec![vec![
            PANEL.owner_before_entry.to_owned(),
            "/usr/local/Kobo/nickel (pid 981, left over)".to_owned(),
        ]];
        assert!(matches!(
            handoff.acquire(&mut driver),
            Err(HandoffError::DoubleOwner { .. })
        ));
        // Nothing was taken: the machine still believes the stock owner holds it.
        assert_eq!(handoff.phase(), Phase::StockOwned);
        assert!(handoff.journal().is_empty());
    }

    #[test]
    fn interrupted_hand_back_rolls_back_and_keeps_diagnostics() {
        let mut handoff = Handoff::new(&PANEL).unwrap();
        let mut driver = Scripted::clean(&PANEL);
        handoff.acquire(&mut driver).unwrap();
        driver.step_failures = vec![None, Some("wpa_supplicant did not exit".to_owned()), None];
        match handoff.hand_back(&mut driver) {
            Err(HandoffError::StepFailed {
                step, diagnostics, ..
            }) => {
                assert_eq!(step, 1);
                assert!(diagnostics.contains("watchdog"));
            }
            other => panic!("expected a step failure, got {other:?}"),
        }
        assert!(driver.rolled_back);
        assert_eq!(handoff.phase(), Phase::RolledBack);
    }

    #[test]
    fn resume_is_proven_by_evidence_not_by_step_success() {
        let mut handoff = Handoff::new(&PANEL).unwrap();
        let mut driver = Scripted::clean(&PANEL);
        handoff.acquire(&mut driver).unwrap();
        driver.resume_observation = Ok("the reader has not reappeared".to_owned());
        assert!(matches!(
            handoff.hand_back(&mut driver),
            Err(HandoffError::ResumeUnproven { .. })
        ));
        assert_ne!(handoff.phase(), Phase::Resumed);
    }

    #[test]
    fn unverified_record_cannot_drive_a_handoff() {
        let record = ownership::wifi_reap(&["/bin/wpa_supplicant"], Evidence::Unverified);
        assert!(matches!(
            Handoff::new(&record),
            Err(HandoffError::UnverifiedRecord)
        ));
        let untouched = ownership::unverified(ResourceKind::Bluetooth);
        assert!(matches!(
            Handoff::new(&untouched),
            Err(HandoffError::UnverifiedRecord)
        ));
    }

    #[test]
    fn every_supported_profile_record_either_drives_or_refuses_loudly() {
        for profile in kobo_profile::SUPPORTED_PROFILES {
            for record in profile.ownership {
                match Handoff::new(record) {
                    Ok(_) => assert_eq!(record.evidence, Evidence::Measured),
                    Err(HandoffError::UnverifiedRecord) => {
                        assert_eq!(record.evidence, Evidence::Unverified);
                    }
                    Err(other) => panic!("{}: unexpected refusal {other}", profile.id),
                }
            }
        }
    }
}
