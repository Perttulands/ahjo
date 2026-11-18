//! Deleting a finished run: its record and what Ahjo made for it.
use crate::Coordinator;
use std::io;
use std::path::PathBuf;

impl Coordinator {
    /// Delete a finished run: its event record, frozen mission, bindings,
    /// kept outputs, the branches its failed attempts' work was kept on and
    /// the folder Ahjo made for it. A folder the operator
    /// gave stays, and a repository run is landed first so its branch is
    /// not lost.
    pub fn delete_run(&self, id: &str) -> Result<(), crate::Error> {
        (|| -> io::Result<()> {
            let run = self.run(id).map_err(crate::error::to_io)?;
            if !run.r#final {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the run is still going; abort it first",
                ));
            }
            if !run.repository.is_empty() && PathBuf::from(&run.cwd).exists() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the run still has its branch and worktree; land it first",
                ));
            }
            // The branches a failed or stopped attempt's work was kept on
            // go with the run, each only while it is where Ahjo left it.
            let events = self.ledger(id)?.read().map_err(crate::error::to_io)?;
            let checkout = events[0].data.get("checkout").and_then(|v| v.as_str());
            if let Some(checkout) = checkout.filter(|c| std::path::Path::new(c).exists()) {
                for kept in events.iter().filter(|e| e.kind == "workspace_kept") {
                    let field = |name: &str| kept.data.get(name).and_then(|v| v.as_str());
                    if let (Some(branch), Some(commit)) = (field("branch"), field("commit")) {
                        let reference = format!("refs/heads/{branch}");
                        // A branch already gone, or moved by someone, stays as it is.
                        let _ = std::process::Command::new("git")
                            .current_dir(checkout)
                            .args(["update-ref", "-d", &reference, commit])
                            .output()?;
                    }
                }
            }
            let root = &self.inner.root;
            let made = self
                .inner
                .settings
                .run_workspace_root
                .clone()
                .unwrap_or_else(|| root.join("workspaces"))
                .join(id);
            // Kept outputs are sealed read-only, so plain removal would fail.
            for dir in [root.join("artifacts").join(id), made] {
                crate::kept_copy::remove_tree(&dir)?;
            }
            crate::SeatRecords::new(root)
                .forget_run(id)
                .map_err(crate::error::to_io)?;
            // The record goes last: until then the run still lists and a
            // failed delete can be tried again.
            for end in ["bindings.toml", "snapshot.toml", "ndjson"] {
                match std::fs::remove_file(root.join("runs").join(format!("{id}.{end}"))) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
            Ok(())
        })()
        .map_err(crate::error::io)
    }
}
