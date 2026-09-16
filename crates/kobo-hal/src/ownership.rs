//! The real ownership driver: probes the device, never infers.
//!
//! Where the profile records declare *what* should own each resource, this
//! layer looks at the hardware and reports what *does*. Process ownership
//! comes from reading `/proc/*/exe` links against the record's
//! `owner_process`; node, interface and adapter claims come from presence
//! probes of the declared nodes (`/dev/pts`, `/sys/class/net/...`,
//! `/dev/stpwmt`). Absence is reported as absence with evidence, never
//! smoothed over.
//!
//! The probe root is a constructor argument: production passes `/`, tests
//! pass a scripted fixture tree, which is how the probe logic is unit-tested
//! without hardware.
//!
//! This driver is deliberately read-only. Actuating hand-back steps (stopping
//! daemons, restarting the reader) stay with the owner-attended kobo-handoff
//! binary behind `device-write`; asking this driver to run one fails loudly
//! rather than pretending. Physical end-to-end evidence remains UNVERIFIED
//! per docs/quality/contracts/device-resource-ownership.md.

use kobo_profile::ownership::{Driver, ResourceOwnership};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// One declared node and whether the probe found it, with the evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeProbe {
    pub node: &'static str,
    pub present: bool,
    /// What the probe actually saw ("pid 814 owns it", "no such node").
    pub evidence: String,
}

/// What a startup probe reports for one resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnershipProbe {
    pub owners: Vec<String>,
    pub nodes: Vec<NodeProbe>,
}

/// A [`Driver`] over the real device (or a fixture tree in tests).
pub struct DeviceDriver<'a> {
    record: &'a ResourceOwnership,
    root: PathBuf,
}

impl<'a> DeviceDriver<'a> {
    #[must_use]
    pub fn new(record: &'a ResourceOwnership, root: impl Into<PathBuf>) -> Self {
        Self {
            record,
            root: root.into(),
        }
    }

    /// The full startup probe: process ownership plus presence of every
    /// declared node. This is the "probe, never infer" entry point a runtime
    /// calls before trusting a profile's expectations.
    #[must_use]
    pub fn probe(&self) -> OwnershipProbe {
        OwnershipProbe {
            owners: self.probe_process_owners(),
            nodes: self
                .record
                .nodes
                .iter()
                .map(|node| self.probe_node(node))
                .collect(),
        }
    }

    /// Every running process whose executable is the record's
    /// `owner_process`, as "<path> (pid <n>)" identities.
    fn probe_process_owners(&self) -> Vec<String> {
        process_owners(&self.root, self.record.owner_process)
    }

    fn probe_node(&self, node: &'static str) -> NodeProbe {
        if let Some(target) = node.strip_prefix("/proc/*/exe -> ") {
            let owners = process_owners(&self.root, target);
            return NodeProbe {
                node,
                present: !owners.is_empty(),
                evidence: if owners.is_empty() {
                    format!("no process runs {target}")
                } else {
                    owners.join(", ")
                },
            };
        }
        let path = self.root.join(node.trim_start_matches('/'));
        // A trailing '*' asks for "any matching entry" (touch event nodes).
        if let Some(prefix) = node.strip_suffix('*') {
            let parent = self.root.join(prefix.trim_start_matches('/'));
            let found = parent
                .parent()
                .and_then(|directory| fs::read_dir(directory).ok());
            let found = found.is_some_and(|entries| {
                let wanted = parent
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                entries
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().starts_with(&wanted))
            });
            return NodeProbe {
                node,
                present: found,
                evidence: if found {
                    format!("a node matching {node} exists")
                } else {
                    format!("no node matching {node}")
                },
            };
        }
        NodeProbe {
            node,
            present: path.exists(),
            evidence: if path.exists() {
                format!("{node} exists")
            } else {
                format!("no such node: {node}")
            },
        }
    }
}

/// Every pid under `root/proc` whose `exe` link resolves to `target`, as
/// "<path> (pid <n>)" identities. An empty target or an unreadable process
/// table yields no owners: absence is reported, never inferred.
fn process_owners(root: &Path, target: &str) -> Vec<String> {
    if target.is_empty() {
        return Vec::new();
    }
    let mut owners = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("proc")) else {
        return owners;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if let Ok(link) = fs::read_link(entry.path().join("exe")) {
            if link == Path::new(target) {
                owners.push(format!("{target} (pid {name})"));
            }
        }
    }
    owners.sort();
    owners
}

impl Driver for DeviceDriver<'_> {
    fn probe_owners(&mut self) -> Result<Vec<String>, String> {
        Ok(self.probe_process_owners())
    }

    fn run_step(&mut self, action: &'static str, _timeout_seconds: u32) -> Result<String, String> {
        Err(format!(
            "{action}: actuating steps stay with the owner-attended kobo-handoff binary; this driver only probes"
        ))
    }

    fn observe_resume(&mut self) -> Result<String, String> {
        let owners = self.probe_process_owners();
        if owners.len() == 1 {
            Ok(self.record.resume_evidence.to_owned())
        } else {
            Err(format!(
                "resume not observable: expected exactly one {} process, found {}",
                self.record.owner_process,
                owners.len()
            ))
        }
    }

    fn rollback(&mut self) -> Result<String, String> {
        Err(
            "rollback actuates the device and stays with the kobo-handoff binary; nothing here changed, so nothing here rolls back"
                .to_owned(),
        )
    }
}

impl fmt::Debug for DeviceDriver<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceDriver")
            .field("resource", &self.record.resource)
            .field("root", &self.root)
            .finish()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use kobo_handoff::Handoff;
    use kobo_profile::ownership::{self, Evidence, ResourceKind};
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A fixture tree: proc entries with exe links, device nodes, interfaces.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "kobo-hal-ownership-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("proc")).unwrap();
            Self(root)
        }

        fn with_process(self, pid: u32, executable: &str) -> Self {
            let directory = self.0.join("proc").join(pid.to_string());
            fs::create_dir_all(&directory).unwrap();
            symlink(executable, directory.join("exe")).unwrap();
            self
        }

        fn with_node(self, relative: &str) -> Self {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, []).unwrap();
            self
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    static PANEL: ResourceOwnership = ownership::panel_via_reader_restart(&[], Evidence::Measured);

    #[test]
    fn probe_finds_the_single_stock_owner_by_its_executable() {
        let fixture = Fixture::new().with_process(814, "/usr/local/Kobo/nickel");
        let driver = DeviceDriver::new(&PANEL, fixture.path());
        let probe = driver.probe();
        assert_eq!(probe.owners, ["/usr/local/Kobo/nickel (pid 814)"]);
        let exe_probe = probe
            .nodes
            .iter()
            .find(|node| node.node.contains("/proc/"))
            .unwrap();
        assert!(exe_probe.present);
        assert!(exe_probe.evidence.contains("pid 814"));
    }

    #[test]
    fn probe_reports_both_owners_when_two_processes_match() {
        let fixture = Fixture::new()
            .with_process(814, "/usr/local/Kobo/nickel")
            .with_process(981, "/usr/local/Kobo/nickel");
        let driver = DeviceDriver::new(&PANEL, fixture.path());
        assert_eq!(driver.probe().owners.len(), 2);
    }

    #[test]
    fn probe_reports_absence_with_evidence_never_silence() {
        let fixture = Fixture::new().with_node("sys/class/net/wlan0");
        let wifi = ownership::wifi_reap(&[], Evidence::Measured);
        let driver = DeviceDriver::new(&wifi, fixture.path());
        let probe = driver.probe();
        let by_node = |name: &str| probe.nodes.iter().find(|n| n.node == name).unwrap();
        assert!(by_node("/sys/class/net/wlan0").present);
        let supplicant = by_node("/var/run/wpa_supplicant");
        assert!(!supplicant.present);
        assert!(supplicant.evidence.contains("no such node"));
    }

    #[test]
    fn acquisition_through_the_real_driver_holds_the_resource() {
        let fixture = Fixture::new().with_process(814, "/usr/local/Kobo/nickel");
        let mut handoff = Handoff::new(&PANEL).unwrap();
        let mut driver = DeviceDriver::new(&PANEL, fixture.path());
        handoff.acquire(&mut driver).unwrap();
        assert_eq!(
            handoff.journal()[0].observation,
            "probed: sole owner is /usr/local/Kobo/nickel (pid 814)"
        );
    }

    #[test]
    fn acquisition_through_the_real_driver_refuses_a_double_owner() {
        let fixture = Fixture::new()
            .with_process(814, "/usr/local/Kobo/nickel")
            .with_process(981, "/usr/local/Kobo/nickel");
        let mut handoff = Handoff::new(&PANEL).unwrap();
        let mut driver = DeviceDriver::new(&PANEL, fixture.path());
        assert!(matches!(
            handoff.acquire(&mut driver),
            Err(kobo_handoff::HandoffError::DoubleOwner { .. })
        ));
    }

    #[test]
    fn resume_is_observed_from_the_process_table() {
        let fixture = Fixture::new().with_process(814, "/usr/local/Kobo/nickel");
        let mut driver = DeviceDriver::new(&PANEL, fixture.path());
        assert_eq!(driver.observe_resume().unwrap(), PANEL.resume_evidence);
        let empty = Fixture::new();
        let mut driver = DeviceDriver::new(&PANEL, empty.path());
        assert!(driver.observe_resume().is_err());
    }

    #[test]
    fn actuating_steps_fail_loudly_instead_of_pretending() {
        let fixture = Fixture::new();
        let mut driver = DeviceDriver::new(&PANEL, fixture.path());
        let error = driver.run_step("stop each leftover radio daemon and wait for exit", 15);
        assert!(error.unwrap_err().contains("kobo-handoff binary"));
    }

    #[test]
    fn glob_nodes_match_any_touch_event_device() {
        let touch = ownership::touch_borrowed(Evidence::Measured);
        let fixture = Fixture::new().with_node("dev/input/event1");
        let driver = DeviceDriver::new(&touch, fixture.path());
        let probe = driver.probe();
        assert!(probe
            .nodes
            .iter()
            .any(|n| n.node == "/dev/input/event*" && n.present));
        let empty = Fixture::new();
        let driver = DeviceDriver::new(&touch, empty.path());
        assert!(driver
            .probe()
            .nodes
            .iter()
            .any(|n| n.node == "/dev/input/event*" && !n.present));
    }

    #[test]
    fn an_untouched_resource_probes_to_nothing() {
        let bluetooth = ownership::unverified(ResourceKind::Bluetooth);
        let fixture = Fixture::new();
        let driver = DeviceDriver::new(&bluetooth, fixture.path());
        let probe = driver.probe();
        assert!(probe.owners.is_empty());
        assert!(probe.nodes.is_empty());
    }
}
