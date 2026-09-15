//! Per-resource ownership records: who owns each hardware resource before
//! Cobalt entry, how Cobalt takes and returns it, and what proves the stock
//! software resumed (docs/quality/contracts/device-resource-ownership.md).
//!
//! A record is a measured fact about one device and firmware, never a guess
//! from a sibling. Where a resource has not been measured on the device the
//! record says [`Evidence::Unverified`] and carries no claims at all: no
//! nodes, no steps, no evidence text. Runtimes must refuse to drive a handoff
//! from an unverified record, which is what keeps "we think it works" from
//! ever reading as "it works".

/// The hardware resources the platform can touch, one owner at a time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    Panel,
    Touch,
    Wifi,
    Bluetooth,
    Audio,
    Usb,
    PowerWatchdog,
    Pty,
}

/// How Cobalt takes a resource from its stock owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Acquisition {
    /// Shares the resource without displacing the owner (read-only touch).
    Borrow,
    /// Stops the owning process for the session and restarts it on exit.
    Stop,
    /// Kills a leftover the stock restart would duplicate (radio daemons).
    Reap,
    /// No stock owner exists; Cobalt initializes the resource itself.
    Initialize,
}

/// How well a record is evidenced on this device and firmware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence {
    /// Owner-attended evidence exists for this device and firmware.
    Measured,
    /// Not measured here. Every claim field stays empty; the record exists
    /// so that "unknown" is stated, not omitted.
    Unverified,
}

/// One bounded hand-back step. A step without a timeout is a hang the owner
/// cannot tell apart from progress, so every step carries its own bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandBackStep {
    pub action: &'static str,
    pub timeout_seconds: u32,
}

/// The ownership contract for one resource on one device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceOwnership {
    pub resource: ResourceKind,
    pub evidence: Evidence,
    /// The process or subsystem owning the resource before Cobalt entry.
    pub owner_before_entry: &'static str,
    /// Process/device nodes whose inspection establishes who owns it.
    pub nodes: &'static [&'static str],
    pub acquisition: Acquisition,
    /// Prerequisite setup, safe to run repeatedly, in order.
    pub prerequisites: &'static [&'static str],
    /// Bounded hand-back steps, in order.
    pub hand_back: &'static [HandBackStep],
    /// The observation that proves the stock software resumed.
    pub resume_evidence: &'static str,
    /// Firmware-specific exceptions to the general behavior.
    pub firmware_exceptions: &'static [&'static str],
    /// What runs when any step fails: rollback plus diagnostic capture.
    pub rollback: &'static str,
}

/// The record set for hardware nobody has measured: every resource
/// unverified. The provisional profile built from an unrecognised device uses
/// this, so no handoff can ever be driven from a guess.
pub const UNMEASURED: &[ResourceOwnership] = &[
    unverified(ResourceKind::Panel),
    unverified(ResourceKind::Touch),
    unverified(ResourceKind::Wifi),
    unverified(ResourceKind::Bluetooth),
    unverified(ResourceKind::Audio),
    unverified(ResourceKind::Usb),
    unverified(ResourceKind::PowerWatchdog),
    unverified(ResourceKind::Pty),
];

/// A resource nobody has measured on this device. The claims stay empty so a
/// reader cannot mistake an unmeasured resource for a supported one.
#[must_use]
pub const fn unverified(resource: ResourceKind) -> ResourceOwnership {
    ResourceOwnership {
        resource,
        evidence: Evidence::Unverified,
        owner_before_entry: "",
        nodes: &[],
        acquisition: Acquisition::Borrow,
        prerequisites: &[],
        hand_back: &[],
        resume_evidence: "",
        firmware_exceptions: &[],
        rollback: "",
    }
}

/// The panel, handed over by stopping the stock reader and restarted on exit
/// under an armed watchdog (the kobo-handoff binary's measured sequence).
#[must_use]
pub const fn panel_via_reader_restart(
    leftover_radio_daemons: &'static [&'static str],
    evidence: Evidence,
) -> ResourceOwnership {
    ResourceOwnership {
        resource: ResourceKind::Panel,
        evidence,
        owner_before_entry: "nickel (the stock reader process)",
        nodes: &["/dev/fb0", "/proc/*/exe -> /usr/local/Kobo/nickel"],
        acquisition: Acquisition::Stop,
        prerequisites: &[
            "capture the reader's exact executable, arguments and environment",
            "arm the detached restart watchdog before any signal is sent",
            "reap the leftover radio daemons named in leftover_radio_daemons",
        ],
        hand_back: &[
            HandBackStep {
                action: "stop each leftover radio daemon and wait for exit",
                timeout_seconds: 15,
            },
            HandBackStep {
                action: "start the captured reader command and wait for it to appear",
                timeout_seconds: 45,
            },
            HandBackStep {
                action: "confirm the reader stays running past the watchdog margin",
                timeout_seconds: 30,
            },
        ],
        resume_evidence:
            "the restarted reader appears in the process table within its grace and stays up",
        firmware_exceptions: leftover_radio_daemons,
        rollback:
            "the armed watchdog restarts the reader unconditionally; the session journal under /tmp records the failing step",
    }
}

/// The digitiser, borrowed read-only: opened without EVIOCGRAB so the stock
/// reader keeps every event, and handed back by closing the descriptor.
#[must_use]
pub const fn touch_borrowed(evidence: Evidence) -> ResourceOwnership {
    ResourceOwnership {
        resource: ResourceKind::Touch,
        evidence,
        owner_before_entry: "nickel (events are shared, never grabbed)",
        nodes: &["/proc/bus/input/devices", "/dev/input/event*"],
        acquisition: Acquisition::Borrow,
        prerequisites: &["open the discovered touch node read-only; never EVIOCGRAB"],
        hand_back: &[HandBackStep {
            action: "close the touch descriptor",
            timeout_seconds: 5,
        }],
        resume_evidence: "nickel never stopped receiving events, so there is nothing to resume",
        firmware_exceptions: &[],
        rollback: "closing the descriptor is the whole rollback; no grab was ever taken",
    }
}

/// Wi-Fi after hand-back: exactly one supplicant owns the interface. The
/// leftover is reaped before the reader restarts, per the measured collision.
#[must_use]
pub const fn wifi_reap(
    leftover_radio_daemons: &'static [&'static str],
    evidence: Evidence,
) -> ResourceOwnership {
    ResourceOwnership {
        resource: ResourceKind::Wifi,
        evidence,
        owner_before_entry: "nickel's wpa_supplicant (or the MediaTek wmt_launcher)",
        nodes: &["/sys/class/net/wlan0", "/var/run/wpa_supplicant", "/dev/stpwmt"],
        acquisition: Acquisition::Reap,
        prerequisites: &["record the running supplicant's exact process identity"],
        hand_back: &[
            HandBackStep {
                action: "kill the exact captured leftover and wait for exit",
                timeout_seconds: 15,
            },
            HandBackStep {
                action: "verify exactly one supplicant owns wlan0 after the reader restarts",
                timeout_seconds: 30,
            },
        ],
        resume_evidence: match evidence {
            Evidence::Measured => {
                "exactly one supplicant process owns wlan0 and the interface associates without an uptime reset"
            }
            Evidence::Unverified => {
                "UNVERIFIED on this device: one-supplicant recovery is measured only where a profile says so"
            }
        },
        firmware_exceptions: leftover_radio_daemons,
        rollback:
            "leave the from-boot supplicant owning the interface; record both process identities in the session journal",
    }
}

/// The consistency rules a profile's ownership records must satisfy. Returns
/// every violation so a test failure names them all at once.
#[must_use]
pub fn check(records: &[ResourceOwnership]) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if records[..index]
            .iter()
            .any(|earlier| earlier.resource == record.resource)
        {
            violations.push(format!(
                "duplicate record for {:?}: one resource, one owner",
                record.resource
            ));
        }
        match record.evidence {
            Evidence::Measured => {
                if record.owner_before_entry.is_empty() {
                    violations.push(format!(
                        "{:?}: measured record names no owner",
                        record.resource
                    ));
                }
                if record.resume_evidence.is_empty() {
                    violations.push(format!(
                        "{:?}: measured record has no resume observation",
                        record.resource
                    ));
                }
                if record.rollback.is_empty() {
                    violations.push(format!(
                        "{:?}: measured record has no rollback",
                        record.resource
                    ));
                }
                for step in record.hand_back {
                    if step.timeout_seconds == 0 || step.timeout_seconds > 300 {
                        violations.push(format!(
                            "{:?}: step {:?} is not bounded ({}s)",
                            record.resource, step.action, step.timeout_seconds
                        ));
                    }
                }
            }
            Evidence::Unverified => {
                // Nodes, prerequisites and steps describe what the runtime
                // does, which is a code fact and allowed. The resume claim is
                // the measured part: without device evidence it must say so
                // out loud rather than read as success.
                let behaves = !record.nodes.is_empty()
                    || !record.prerequisites.is_empty()
                    || !record.hand_back.is_empty();
                if behaves && !record.resume_evidence.starts_with("UNVERIFIED") {
                    violations.push(format!(
                        "{:?}: unverified behavior must say UNVERIFIED in its resume claim",
                        record.resource
                    ));
                }
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SUPPORTED_PROFILES;

    #[test]
    fn every_supported_profile_satisfies_the_ownership_invariants() {
        for profile in SUPPORTED_PROFILES {
            let violations = check(profile.ownership);
            assert!(
                violations.is_empty(),
                "{}: {}",
                profile.id,
                violations.join("; ")
            );
        }
    }

    #[test]
    fn every_supported_profile_records_panel_touch_and_wifi() {
        for profile in SUPPORTED_PROFILES {
            for resource in [ResourceKind::Panel, ResourceKind::Touch, ResourceKind::Wifi] {
                assert!(
                    profile
                        .ownership
                        .iter()
                        .any(|record| record.resource == resource),
                    "{}: no ownership record for {:?}",
                    profile.id,
                    resource
                );
            }
        }
    }

    #[test]
    fn unverified_behavior_must_say_so_in_its_resume_claim() {
        let mut record = wifi_reap(&["/bin/wpa_supplicant"], Evidence::Unverified);
        record.resume_evidence = "associates cleanly after hand-back";
        assert!(!check(&[record]).is_empty());
        record.resume_evidence = "UNVERIFIED on this device: no attended evidence";
        assert!(check(&[record]).is_empty());
    }

    #[test]
    fn an_untouched_unverified_resource_carries_no_claims() {
        assert!(check(&[unverified(ResourceKind::Bluetooth)]).is_empty());
    }

    #[test]
    fn a_measured_step_without_a_timeout_is_rejected() {
        let mut record = touch_borrowed(Evidence::Measured);
        record.hand_back = &[HandBackStep {
            action: "wait forever",
            timeout_seconds: 0,
        }];
        assert!(!check(&[record]).is_empty());
    }
}
