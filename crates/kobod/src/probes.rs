//! Startup composition of the ownership records into availability evidence.
//!
//! The records declare what should own each resource; the driver probes what
//! actually does (docs/quality/contracts/device-resource-ownership.md). The
//! runtime cites what the probe saw when it reports a capability
//! unsupported, so the answer carries evidence instead of an assumption.

use kobo_policy::Capability;
use kobo_profile::ownership::{ResourceKind, ResourceOwnership};
use std::path::Path;

/// The capabilities a resource's absence takes with it.
#[must_use]
pub fn capabilities_of(resource: ResourceKind) -> &'static [Capability] {
    match resource {
        ResourceKind::Wifi => &[Capability::WifiControl, Capability::Network],
        ResourceKind::Bluetooth => &[Capability::BluetoothControl],
        ResourceKind::Audio => &[Capability::Audio, Capability::BluetoothAudio],
        ResourceKind::Panel
        | ResourceKind::Touch
        | ResourceKind::Usb
        | ResourceKind::PowerWatchdog
        | ResourceKind::Pty => &[],
    }
}

/// Probes every record under `root` (`/` on a reader, a fixture tree in
/// tests) and returns, per capability, what the probe saw: the evidence the
/// runtime cites when it reports the capability unsupported. A record whose
/// claims all hold yields nothing, and an unverified record declares no
/// claims, so it stays silent rather than guessing.
#[must_use]
pub fn probe_evidence(records: &[ResourceOwnership], root: &Path) -> Vec<(Capability, String)> {
    let mut found = Vec::new();
    for record in records {
        let probe = kobo_hal::ownership::DeviceDriver::new(record, root).probe();
        let mut parts: Vec<String> = probe
            .nodes
            .iter()
            .filter(|node| !node.present)
            .map(|node| node.evidence.clone())
            .collect();
        if probe.owners.is_empty() && !record.owner_process.is_empty() {
            parts.push(format!("no process runs {}", record.owner_process));
        }
        if parts.is_empty() {
            continue;
        }
        let text = parts.join("; ");
        for capability in capabilities_of(record.resource) {
            found.push((*capability, text.clone()));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobo_profile::ownership::{Acquisition, Evidence};

    const WIFI_RECORD: ResourceOwnership = ResourceOwnership {
        resource: ResourceKind::Wifi,
        evidence: Evidence::Measured,
        owner_before_entry: "the stock wireless daemon",
        owner_process: "",
        nodes: &["/sys/class/net/wlan0"],
        acquisition: Acquisition::Reap,
        prerequisites: &[],
        hand_back: &[],
        resume_evidence: "",
        firmware_exceptions: &[],
        rollback: "",
    };

    #[test]
    fn absence_is_cited_per_capability_and_unverified_records_stay_silent() {
        let root = std::env::temp_dir().join(format!("kobod-probes-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture root");
        // The node is absent from the fixture tree: the probe's own words
        // are cited against both capabilities the resource carries.
        let found = probe_evidence(&[WIFI_RECORD], &root);
        assert_eq!(found.len(), 2);
        for (capability, text) in &found {
            assert!(matches!(
                capability,
                Capability::WifiControl | Capability::Network
            ));
            assert!(
                text.contains("no such node: /sys/class/net/wlan0"),
                "{text}"
            );
        }
        // Present nodes yield no evidence: nothing unsupported to explain.
        std::fs::create_dir_all(root.join("sys/class/net/wlan0")).expect("fixture node");
        assert!(probe_evidence(&[WIFI_RECORD], &root).is_empty());
        // An unverified record declares no claims, so it says nothing.
        assert!(probe_evidence(
            &[kobo_profile::ownership::unverified(ResourceKind::Bluetooth)],
            &root,
        )
        .is_empty());
        let _ignored = std::fs::remove_dir_all(&root);
    }
}
