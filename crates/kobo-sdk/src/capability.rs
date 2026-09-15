//! Reasoned capability availability.
//!
//! Replaces the boolean capable/incapable reading with the state the
//! evidence actually carries (issues #90 and #55): a device that needs
//! owner setup is never reported as unsupported hardware, a missing
//! prerequisite is never inferred from the device profile alone, and an
//! `unsupported` report carries the probe evidence rather than an
//! assumption. Profiles declare expectations; probes decide.
//!
//! Contract: docs/quality/contracts/capability-availability.md.

use kobo_protocol::{CapabilityAvailability, DeviceResult};

/// The availability state a capability report carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityState {
    /// Usable now.
    Available,
    /// Usable after a named owner action, such as enabling Wi-Fi in
    /// Nickel or pairing headphones.
    OwnerSetupRequired,
    /// Transiently unusable: a radio is off, or Nickel owns the
    /// resource right now.
    TemporarilyUnavailable,
    /// The user or policy refused it.
    Denied,
    /// The hardware or firmware genuinely lacks it.
    Unsupported,
}

/// A capability report: the state plus the human-readable reason that
/// names the owner action, the change that would help, the permission,
/// or the probe evidence. There is no state without its reason: the
/// constructors take it, so a report cannot forget to say why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    state: CapabilityState,
    reason: String,
}

impl Capability {
    /// Usable now.
    #[must_use]
    pub fn available() -> Self {
        Self {
            state: CapabilityState::Available,
            reason: "available".to_owned(),
        }
    }

    /// Usable after the named owner action.
    pub fn owner_setup_required(action: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::OwnerSetupRequired,
            reason: action.into(),
        }
    }

    /// Transiently unusable; the reason says what would change it.
    pub fn temporarily_unavailable(change: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::TemporarilyUnavailable,
            reason: change.into(),
        }
    }

    /// Refused by the user or policy; the reason names the permission
    /// and where to change it.
    pub fn denied(permission: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Denied,
            reason: permission.into(),
        }
    }

    /// Genuinely absent; the reason is the probe evidence, never an
    /// assumption from the device profile.
    pub fn unsupported(evidence: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Unsupported,
            reason: evidence.into(),
        }
    }

    /// The state this report carries.
    #[must_use]
    pub fn state(&self) -> CapabilityState {
        self.state
    }

    /// Why: the owner action, the change, the permission, or the probe
    /// evidence, in the device's own words.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The derived boolean view: usable now. A view, never the source
    /// of truth - the state and reason are.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.state == CapabilityState::Available
    }

    /// The typed reading of a wire answer, or `None` when the answer was
    /// about something else. Pair with
    /// [`crate::Device::read_capability`]: the request asks, the runtime
    /// answers, and this keeps the answer in states and reasons instead
    /// of a boolean.
    #[must_use]
    pub fn report(result: &DeviceResult) -> Option<Self> {
        let DeviceResult::Capability { state, reason, .. } = result else {
            return None;
        };
        Some(match state {
            CapabilityAvailability::Available => Self::available(),
            CapabilityAvailability::OwnerSetupRequired => {
                Self::owner_setup_required(reason.clone())
            }
            CapabilityAvailability::TemporarilyUnavailable => {
                Self::temporarily_unavailable(reason.clone())
            }
            CapabilityAvailability::Denied => Self::denied(reason.clone()),
            CapabilityAvailability::Unsupported => Self::unsupported(reason.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Capability, CapabilityState};
    use kobo_protocol::{CapabilityAvailability, DeviceResult};

    #[test]
    fn a_wire_answer_reads_back_as_states_and_reasons() {
        let cases = [
            (
                CapabilityAvailability::Available,
                CapabilityState::Available,
            ),
            (
                CapabilityAvailability::OwnerSetupRequired,
                CapabilityState::OwnerSetupRequired,
            ),
            (
                CapabilityAvailability::TemporarilyUnavailable,
                CapabilityState::TemporarilyUnavailable,
            ),
            (CapabilityAvailability::Denied, CapabilityState::Denied),
            (
                CapabilityAvailability::Unsupported,
                CapabilityState::Unsupported,
            ),
        ];
        for (wire, expected) in cases {
            let result = DeviceResult::Capability {
                name: "wifi".to_owned(),
                state: wire,
                reason: "the reason in the device's own words".to_owned(),
            };
            let report = Capability::report(&result).expect("a capability report");
            assert_eq!(report.state(), expected);
            if expected == CapabilityState::Available {
                assert_eq!(report.reason(), "available");
            } else {
                assert_eq!(report.reason(), "the reason in the device's own words");
            }
        }
        assert!(Capability::report(&DeviceResult::Done).is_none());
    }

    #[test]
    fn every_state_carries_its_reason() {
        let cases = [
            (Capability::available(), CapabilityState::Available, "available"),
            (
                Capability::owner_setup_required("enable Wi-Fi in Nickel"),
                CapabilityState::OwnerSetupRequired,
                "enable Wi-Fi in Nickel",
            ),
            (
                Capability::temporarily_unavailable("the radio is off"),
                CapabilityState::TemporarilyUnavailable,
                "the radio is off",
            ),
            (
                Capability::denied("network access, in the app's permissions"),
                CapabilityState::Denied,
                "network access, in the app's permissions",
            ),
            (
                Capability::unsupported("no audio adapter answered the probe"),
                CapabilityState::Unsupported,
                "no audio adapter answered the probe",
            ),
        ];
        for (capability, state, reason) in cases {
            assert_eq!(capability.state(), state);
            assert_eq!(capability.reason(), reason);
            assert_eq!(capability.is_available(), state == CapabilityState::Available);
        }
    }

    #[test]
    fn the_boolean_view_is_derived_never_stored() {
        // Only Available reads as usable; every other state, whatever its
        // reason, is not usable now and says why instead.
        assert!(Capability::available().is_available());
        for capability in [
            Capability::owner_setup_required("pair headphones"),
            Capability::temporarily_unavailable("Nickel owns the radio"),
            Capability::denied("bluetooth permission"),
            Capability::unsupported("no adapter node exists"),
        ] {
            assert!(!capability.is_available());
        }
    }
}
