//! Crash quarantine for hosted applications.
//!
//! An application that dies on its own - a panic, a signal, a failed
//! allocation - takes the count here, never the reader. Five crashes in a
//! row quarantine the application: it stops being launched, and the owner
//! is offered the way out in plain terms - launch without state, export
//! state, reset state, or remove the app. A clean exit resets the count,
//! because one bad afternoon is not a verdict.
//!
//! The ledger is one small file per application on the book partition,
//! written synced, because the crash it records may be the last thing the
//! session does. A ledger that cannot be parsed reads as zero and is
//! rewritten on the next record: a corrupt record must never trap an
//! application that is actually healthy, and the next crash simply starts
//! the count again.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Consecutive crashes that quarantine an application. Chosen to be well
/// past an unlucky restart and well short of a reader stuck in a crash
/// loop: five launches, five deaths, no successful run between them.
pub const CONSECUTIVE_CRASH_LIMIT: u32 = 5;

/// Per-application crash records, rooted beside the rest of Cobalt's state.
pub struct Health {
    root: PathBuf,
}

/// What a recovery choice does with the application's saved state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recovery {
    /// State moved aside; the next launch starts clean and the aside copy
    /// stays on the partition until the owner removes it.
    LaunchWithoutState,
    /// State copied into the exports directory, untouched at the source.
    ExportState,
    /// State deleted outright.
    ResetState,
}

impl Health {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn ledger(&self, application: &str) -> Result<PathBuf, String> {
        checked_name(application).map(|name| self.root.join("health").join(name))
    }

    fn state_dir(&self, application: &str) -> Result<PathBuf, String> {
        checked_name(application).map(|name| self.root.join("state").join(name))
    }

    /// The recorded consecutive crash count. An absent or unreadable ledger
    /// is zero: the count is evidence, not suspicion.
    pub fn crashes(&self, application: &str) -> u32 {
        let Ok(path) = self.ledger(application) else {
            return 0;
        };
        fs::read_to_string(path)
            .ok()
            .and_then(|text| parse_crashes(&text))
            .unwrap_or(0)
    }

    /// Whether the application is quarantined: the limit reached, no clean
    /// exit since.
    pub fn is_quarantined(&self, application: &str) -> bool {
        self.crashes(application) >= CONSECUTIVE_CRASH_LIMIT
    }

    /// A launch that ran and ended normally clears the count.
    pub fn record_clean_exit(&self, application: &str) -> Result<(), String> {
        let path = self.ledger(application)?;
        if self.crashes(application) == 0 && !path.exists() {
            return Ok(());
        }
        write_ledger(&path, 0)
    }

    /// An application that died on its own. Returns the new count.
    pub fn record_crash(&self, application: &str) -> Result<u32, String> {
        let path = self.ledger(application)?;
        let crashes = self.crashes(application).saturating_add(1);
        write_ledger(&path, crashes)?;
        Ok(crashes)
    }

    /// Clears the record after a recovery choice carried it out.
    pub fn release(&self, application: &str) -> Result<(), String> {
        let path = self.ledger(application)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|error| format!("clear {}: {error}", path.display()))?;
        }
        Ok(())
    }

    /// Carries out the state half of a recovery choice. Returns what was
    /// done with the state, in words the confirmation screen can repeat.
    pub fn recover(
        &self,
        application: &str,
        choice: Recovery,
        exports: &Path,
    ) -> Result<String, String> {
        let state = self.state_dir(application)?;
        match choice {
            Recovery::LaunchWithoutState => {
                if state.exists() {
                    let aside = free_name(&state, "held")?;
                    fs::rename(&state, &aside).map_err(|error| {
                        format!("move {} aside: {error}", state.display())
                    })?;
                    Ok(format!("state set aside at {}", aside.display()))
                } else {
                    Ok("no saved state to set aside".to_owned())
                }
            }
            Recovery::ExportState => {
                if !state.exists() {
                    return Ok("no saved state to export".to_owned());
                }
                fs::create_dir_all(exports)
                    .map_err(|error| format!("create {}: {error}", exports.display()))?;
                let destination = free_name(&exports.join(format!("{application}-state")), "")?;
                copy_directory(&state, &destination)?;
                Ok(format!("state exported to {}", destination.display()))
            }
            Recovery::ResetState => {
                if state.exists() {
                    fs::remove_dir_all(&state)
                        .map_err(|error| format!("remove {}: {error}", state.display()))?;
                    Ok("saved state deleted".to_owned())
                } else {
                    Ok("no saved state to delete".to_owned())
                }
            }
        }
    }
}

/// Application names come from package manifests and end up in paths. Only
/// a plain slug may reach the filesystem.
fn checked_name(application: &str) -> Result<&str, String> {
    let plain = !application.is_empty()
        && application
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    if plain {
        Ok(application)
    } else {
        Err(format!("{application:?} is not an application name"))
    }
}

fn parse_crashes(text: &str) -> Option<u32> {
    let line = text.lines().next()?;
    line.strip_prefix("crashes=")?.parse().ok()
}

fn write_ledger(path: &Path, crashes: u32) -> Result<(), String> {
    let parent = path.parent().ok_or("ledger has no parent")?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    let mut file =
        fs::File::create(path).map_err(|error| format!("write {}: {error}", path.display()))?;
    file.write_all(format!("crashes={crashes}\n").as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("write {}: {error}", path.display()))
}

/// The first free deterministic suffix: `name.held`, then `name.held.2`,
/// `name.held.3`, and so on. Recovery must never overwrite what an earlier
/// recovery preserved.
fn free_name(base: &Path, marker: &str) -> Result<PathBuf, String> {
    let named = |suffix: &str| {
        let mut name = base.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    let first = if marker.is_empty() {
        base.to_path_buf()
    } else {
        named(&format!(".{marker}"))
    };
    if !first.exists() {
        return Ok(first);
    }
    for index in 2.. {
        let candidate = if marker.is_empty() {
            named(&format!("-{index}"))
        } else {
            named(&format!(".{marker}.{index}"))
        };
        if !candidate.exists() {
            return Ok(candidate);
        }
        if index > 1000 {
            return Err(format!("no free recovery name near {}", base.display()));
        }
    }
    unreachable!()
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination)
        .map_err(|error| format!("create {}: {error}", destination.display()))?;
    let entries = fs::read_dir(source)
        .map_err(|error| format!("read {}: {error}", source.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read {}: {error}", source.display()))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|error| format!("inspect {}: {error}", from.display()))?;
        if kind.is_dir() {
            copy_directory(&from, &to)?;
        } else if kind.is_file() {
            fs::copy(&from, &to)
                .map_err(|error| format!("copy {}: {error}", from.display()))?;
        }
        // Anything that is neither a file nor a directory - a symlink, a
        // socket - is skipped rather than followed out of the state tree.
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let folder = std::env::current_dir()
                .expect("working directory")
                .join("target")
                .join(format!("kobod-health-{name}-{}", std::process::id()));
            let _ignored = fs::remove_dir_all(&folder);
            fs::create_dir_all(&folder).expect("scratch folder");
            Self(folder)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn five_consecutive_crashes_quarantine_and_a_clean_exit_resets() {
        let scratch = Scratch::new("threshold");
        let health = Health::new(&scratch.0);
        assert!(!health.is_quarantined("word-count"));
        for round in 1..CONSECUTIVE_CRASH_LIMIT {
            assert_eq!(health.record_crash("word-count").unwrap(), round);
            assert!(!health.is_quarantined("word-count"), "quarantined at {round}");
        }
        assert_eq!(
            health.record_crash("word-count").unwrap(),
            CONSECUTIVE_CRASH_LIMIT
        );
        assert!(health.is_quarantined("word-count"));
        health.record_clean_exit("word-count").unwrap();
        assert_eq!(health.crashes("word-count"), 0);
        assert!(!health.is_quarantined("word-count"));
    }

    #[test]
    fn crashes_must_be_consecutive() {
        let scratch = Scratch::new("consecutive");
        let health = Health::new(&scratch.0);
        for _ in 0..4 {
            health.record_crash("reader").unwrap();
        }
        health.record_clean_exit("reader").unwrap();
        for _ in 0..4 {
            health.record_crash("reader").unwrap();
        }
        assert!(!health.is_quarantined("reader"));
    }

    #[test]
    fn a_corrupt_ledger_reads_as_zero_and_heals_on_the_next_record() {
        let scratch = Scratch::new("corrupt-ledger");
        let health = Health::new(&scratch.0);
        let ledger = scratch.0.join("health/todo");
        fs::create_dir_all(ledger.parent().unwrap()).unwrap();
        fs::write(&ledger, "crashes=many\n").unwrap();
        assert_eq!(health.crashes("todo"), 0);
        assert!(!health.is_quarantined("todo"));
        assert_eq!(health.record_crash("todo").unwrap(), 1);
        assert_eq!(fs::read_to_string(&ledger).unwrap(), "crashes=1\n");
    }

    #[test]
    fn release_clears_the_record() {
        let scratch = Scratch::new("release");
        let health = Health::new(&scratch.0);
        for _ in 0..CONSECUTIVE_CRASH_LIMIT {
            health.record_crash("todo").unwrap();
        }
        assert!(health.is_quarantined("todo"));
        health.release("todo").unwrap();
        assert_eq!(health.crashes("todo"), 0);
    }

    #[test]
    fn names_that_are_not_slugs_never_reach_the_filesystem() {
        let scratch = Scratch::new("names");
        let health = Health::new(&scratch.0);
        for bad in ["../cobalt", "a/b", "", "with space", "dot.dot"] {
            assert!(health.record_crash(bad).is_err(), "{bad}");
            assert_eq!(health.crashes(bad), 0, "{bad}");
        }
        assert!(!scratch.0.join("health").exists() || fs::read_dir(scratch.0.join("health")).unwrap().next().is_none());
    }

    #[test]
    fn launch_without_state_sets_state_aside_without_losing_it() {
        let scratch = Scratch::new("aside");
        let health = Health::new(&scratch.0);
        let state = scratch.0.join("state/todo");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("items"), "buy milk\n").unwrap();
        let outcome = health
            .recover("todo", Recovery::LaunchWithoutState, &scratch.0.join("exports"))
            .unwrap();
        assert!(outcome.contains("set aside"), "{outcome}");
        assert!(!state.exists());
        assert_eq!(
            fs::read_to_string(scratch.0.join("state/todo.held/items")).unwrap(),
            "buy milk\n"
        );
        // A second quarantine never overwrites the first aside copy.
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("items"), "buy eggs\n").unwrap();
        health
            .recover("todo", Recovery::LaunchWithoutState, &scratch.0.join("exports"))
            .unwrap();
        assert_eq!(
            fs::read_to_string(scratch.0.join("state/todo.held.2/items")).unwrap(),
            "buy eggs\n"
        );
    }

    #[test]
    fn export_copies_state_and_names_conflicts_deterministically() {
        let scratch = Scratch::new("export");
        let health = Health::new(&scratch.0);
        let state = scratch.0.join("state/reader");
        fs::create_dir_all(state.join("notes")).unwrap();
        fs::write(state.join("notes/one"), "first\n").unwrap();
        fs::write(state.join("position"), "42\n").unwrap();
        let exports = scratch.0.join("exports");
        let outcome = health.recover("reader", Recovery::ExportState, &exports).unwrap();
        assert!(outcome.contains("exported"), "{outcome}");
        assert_eq!(
            fs::read_to_string(exports.join("reader-state/notes/one")).unwrap(),
            "first\n"
        );
        // The source is untouched: an export never mutates the original.
        assert_eq!(fs::read_to_string(state.join("position")).unwrap(), "42\n");
        health.recover("reader", Recovery::ExportState, &exports).unwrap();
        assert!(exports.join("reader-state-2").is_dir());
    }

    #[test]
    fn reset_removes_state_and_recovers_from_corrupt_contents() {
        let scratch = Scratch::new("reset");
        let health = Health::new(&scratch.0);
        let state = scratch.0.join("state/reader");
        fs::create_dir_all(&state).unwrap();
        // A torn write leaves whatever it leaves; reset owes no parse.
        fs::write(state.join("position"), [0xff, 0x00, 0x41]).unwrap();
        let outcome = health
            .recover("reader", Recovery::ResetState, &scratch.0.join("exports"))
            .unwrap();
        assert!(outcome.contains("deleted"), "{outcome}");
        assert!(!state.exists());
        // Nothing to remove is still a carried-out choice.
        let outcome = health
            .recover("reader", Recovery::ResetState, &scratch.0.join("exports"))
            .unwrap();
        assert!(outcome.contains("no saved state"), "{outcome}");
    }

    #[test]
    fn export_and_aside_tolerate_state_that_is_not_there() {
        let scratch = Scratch::new("absent");
        let health = Health::new(&scratch.0);
        let exports = scratch.0.join("exports");
        let outcome = health.recover("ghost", Recovery::ExportState, &exports).unwrap();
        assert!(outcome.contains("no saved state"), "{outcome}");
        let outcome = health
            .recover("ghost", Recovery::LaunchWithoutState, &exports)
            .unwrap();
        assert!(outcome.contains("no saved state"), "{outcome}");
    }
}
