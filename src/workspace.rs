//! # Real Workspace Scanner (CCOS v0.3)
//!
//! Replaces the synthetic in-memory workspace with a scanner over a real
//! directory tree. It detects added / modified / removed `.rs` files and feeds
//! **only the delta** to the [`IncrementalGraphEngine`](crate::incremental),
//! preserving the `O(Δ)` property end-to-end.
//!
//! File I/O is async (`tokio::fs`). The scanner is resilient: a file that
//! disappears mid-scan is treated as removed rather than causing an error.

use crate::incremental::IncrementalGraphEngine;
use crate::memory::MemoryGraph;
use crate::util::sha256_hex as hash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// Hard limits for one workspace scan. They keep an untrusted tree from
/// turning discovery into an unbounded allocation or read.
pub const MAX_WORKSPACE_RUST_FILES: usize = 10_000;
pub const MAX_RUST_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_WORKSPACE_RUST_BYTES: u64 = 512 * 1024 * 1024;

/// Typed error for workspace operations.
#[derive(Debug)]
pub enum WorkspaceError {
    Io(String),
    UnsafePath(String),
    LimitExceeded(String),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::Io(e) => write!(f, "workspace I/O error: {e}"),
            WorkspaceError::UnsafePath(e) => write!(f, "unsafe workspace path: {e}"),
            WorkspaceError::LimitExceeded(e) => write!(f, "workspace limit exceeded: {e}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<std::io::Error> for WorkspaceError {
    fn from(e: std::io::Error) -> Self {
        WorkspaceError::Io(e.to_string())
    }
}

/// The set of changes detected between two scans.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDelta {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
}

impl WorkspaceDelta {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }
    pub fn changed_count(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len()
    }
}

/// Tracks the last-known content hash of every `.rs` file under `root`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceScanner {
    pub root: String,
    pub files: HashMap<String, String>,
}

impl WorkspaceScanner {
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            files: HashMap::new(),
        }
    }

    /// Re-scan the workspace and return the delta versus the previous scan,
    /// updating the internal file index. Does not touch any graph.
    pub async fn scan_workspace(&mut self) -> Result<WorkspaceDelta, WorkspaceError> {
        let current = self.read_current().await?;
        let delta = Self::diff(&self.files, &current);
        self.files = current.into_iter().map(|(p, (h, _))| (p, h)).collect();
        Ok(delta)
    }

    /// Alias of [`Self::scan_workspace`] — the delta since the
    /// last scan.
    pub async fn update_delta(&mut self) -> Result<WorkspaceDelta, WorkspaceError> {
        self.scan_workspace().await
    }

    /// Scan **and** apply the delta to `graph` via the incremental engine:
    /// added/modified files are (re)parsed, removed files are evicted. Only the
    /// changed files touch the engine. Returns the applied delta.
    pub async fn sync(
        &mut self,
        engine: &mut IncrementalGraphEngine,
        graph: &mut MemoryGraph,
    ) -> Result<WorkspaceDelta, WorkspaceError> {
        let current = self.read_current().await?;
        let delta = Self::diff(&self.files, &current);

        for path in delta.added.iter().chain(delta.modified.iter()) {
            if let Some((_, content)) = current.get(path) {
                // Evict any prior version, then ingest — O(Δ): only this file.
                engine.evict_file_nodes(path, graph);
                engine.process_delta(path, None, content, graph);
            }
        }
        for path in &delta.removed {
            engine.evict_file_nodes(path, graph);
            engine.file_states.remove(path);
        }

        self.files = current.into_iter().map(|(p, (h, _))| (p, h)).collect();
        Ok(delta)
    }

    /// Poll the workspace up to `max_polls` times at `interval`, invoking
    /// `on_change` for every non-empty delta. Returns the number of change
    /// events observed. A bounded, crash-free alternative to OS file watching.
    pub async fn watch_changes<F: FnMut(&WorkspaceDelta)>(
        &mut self,
        interval: Duration,
        max_polls: usize,
        mut on_change: F,
    ) -> Result<usize, WorkspaceError> {
        let mut events = 0;
        for i in 0..max_polls {
            if i > 0 {
                tokio::time::sleep(interval).await;
            }
            let delta = self.scan_workspace().await?;
            if !delta.is_empty() {
                events += 1;
                on_change(&delta);
            }
        }
        Ok(events)
    }

    /// Read every `.rs` file under `root` into `(path -> (hash, content))`.
    /// Files that vanish mid-scan are skipped (and thus appear as removed).
    async fn read_current(&self) -> Result<HashMap<String, (String, String)>, WorkspaceError> {
        let mut current = HashMap::new();
        let (root, paths) = Self::collect_rs(&self.root).await?;
        let mut total_bytes = 0u64;
        for path in paths {
            let key = path.to_string_lossy().to_string();
            let Some(content) = Self::read_regular_file(&root, &path, &mut total_bytes).await?
            else {
                continue; // disappeared mid-scan: treat as absent
            };
            current.insert(key, (hash(&content), content));
        }
        Ok(current)
    }

    /// Open one file without following its final symlink, revalidate that it is
    /// a regular file confined to `root`, and read at most the configured
    /// per-file and aggregate budgets. `None` means the file vanished during
    /// the scan.
    async fn read_regular_file(
        root: &Path,
        path: &Path,
        total_bytes: &mut u64,
    ) -> Result<Option<String>, WorkspaceError> {
        let before = match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if before.file_type().is_symlink() || !before.file_type().is_file() {
            return Err(WorkspaceError::UnsafePath(format!(
                "refusing non-regular Rust source {}",
                path.display()
            )));
        }

        let canonical = match tokio::fs::canonicalize(path).await {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !canonical.starts_with(root) {
            return Err(WorkspaceError::UnsafePath(format!(
                "Rust source escapes workspace root: {}",
                path.display()
            )));
        }

        let mut options = tokio::fs::OpenOptions::new();
        options.read(true);
        // Reject a final-component symlink even if it is swapped in after the
        // metadata check. Directory symlinks are never traversed by collect_rs.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        options.custom_flags(0x0002_0000); // O_NOFOLLOW
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        options.custom_flags(0x0000_0100); // O_NOFOLLOW
        #[cfg(windows)]
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT

        let file = match options.open(path).await {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(WorkspaceError::UnsafePath(format!(
                    "cannot safely open {}: {e}",
                    path.display()
                )))
            }
        };
        let metadata = file.metadata().await?;
        if !metadata.file_type().is_file() {
            return Err(WorkspaceError::UnsafePath(format!(
                "refusing non-regular Rust source {}",
                path.display()
            )));
        }
        if metadata.len() > MAX_RUST_FILE_BYTES {
            return Err(WorkspaceError::LimitExceeded(format!(
                "{} is {} bytes (maximum {})",
                path.display(),
                metadata.len(),
                MAX_RUST_FILE_BYTES
            )));
        }
        *total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| WorkspaceError::LimitExceeded("byte budget overflow".into()))?;
        if *total_bytes > MAX_WORKSPACE_RUST_BYTES {
            return Err(WorkspaceError::LimitExceeded(format!(
                "Rust sources exceed {} bytes",
                MAX_WORKSPACE_RUST_BYTES
            )));
        }

        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_RUST_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 > MAX_RUST_FILE_BYTES {
            return Err(WorkspaceError::LimitExceeded(format!(
                "{} grew beyond {} bytes while being read",
                path.display(),
                MAX_RUST_FILE_BYTES
            )));
        }
        let content = String::from_utf8(bytes).map_err(|e| {
            WorkspaceError::Io(format!("{} is not valid UTF-8: {e}", path.display()))
        })?;
        Ok(Some(content))
    }

    fn diff(
        previous: &HashMap<String, String>,
        current: &HashMap<String, (String, String)>,
    ) -> WorkspaceDelta {
        let mut delta = WorkspaceDelta::default();
        for (path, (hash, _)) in current {
            match previous.get(path) {
                None => delta.added.push(path.clone()),
                Some(old) if old != hash => delta.modified.push(path.clone()),
                _ => {}
            }
        }
        for path in previous.keys() {
            if !current.contains_key(path) {
                delta.removed.push(path.clone());
            }
        }
        delta.added.sort();
        delta.modified.sort();
        delta.removed.sort();
        delta
    }

    /// Iteratively (no async recursion) collect `.rs` files, skipping
    /// `target/`, VCS and hidden directories.
    async fn collect_rs(root: &str) -> Result<(PathBuf, Vec<PathBuf>), WorkspaceError> {
        let root_path = PathBuf::from(root);
        let root_metadata = match tokio::fs::symlink_metadata(&root_path).await {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((root_path, Vec::new()))
            }
            Err(e) => return Err(e.into()),
        };
        if root_metadata.file_type().is_symlink() || !root_metadata.file_type().is_dir() {
            return Err(WorkspaceError::UnsafePath(format!(
                "workspace root must be a real directory: {}",
                root_path.display()
            )));
        }
        let root = tokio::fs::canonicalize(&root_path).await?;
        let mut out = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let mut rd = match tokio::fs::read_dir(&dir).await {
                Ok(rd) => rd,
                Err(_) => continue, // directory vanished: skip
            };
            while let Some(entry) = rd.next_entry().await? {
                let path = entry.path();
                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if file_type.is_symlink() {
                    if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                        return Err(WorkspaceError::UnsafePath(format!(
                            "refusing symbolic-link Rust source {}",
                            path.display()
                        )));
                    }
                    continue;
                } else if file_type.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name == "target" || name == ".git" || name.starts_with('.') {
                        continue;
                    }
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    if out.len() == MAX_WORKSPACE_RUST_FILES {
                        return Err(WorkspaceError::LimitExceeded(format!(
                            "more than {MAX_WORKSPACE_RUST_FILES} Rust sources"
                        )));
                    }
                    out.push(path);
                }
            }
        }
        out.sort();
        Ok((root, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "ccos_ws_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn scan_detects_add_modify_remove() {
        let dir = temp_dir("scan");
        let main = dir.join("main.rs");
        let lib = dir.join("lib.rs");
        let module = dir.join("module.rs");
        std::fs::write(&main, "fn main() {}").unwrap();
        std::fs::write(&lib, "pub mod module;").unwrap();
        std::fs::write(&module, "pub fn a() {}").unwrap();

        let mut scanner = WorkspaceScanner::new(dir.to_string_lossy().to_string());

        // First scan: all three are new.
        let d0 = scanner.scan_workspace().await.unwrap();
        assert_eq!(d0.added.len(), 3);
        assert!(d0.modified.is_empty() && d0.removed.is_empty());

        // Second scan, no change.
        let d1 = scanner.scan_workspace().await.unwrap();
        assert!(d1.is_empty(), "no change must produce an empty delta");

        // Modify only module.rs → only module.rs is reported modified.
        std::fs::write(&module, "pub fn a() {}\npub fn b() {}").unwrap();
        let d2 = scanner.scan_workspace().await.unwrap();
        assert_eq!(d2.modified.len(), 1);
        assert!(d2.modified[0].ends_with("module.rs"));
        assert!(d2.added.is_empty() && d2.removed.is_empty());

        // Remove lib.rs.
        std::fs::remove_file(&lib).unwrap();
        let d3 = scanner.scan_workspace().await.unwrap();
        assert_eq!(d3.removed.len(), 1);
        assert!(d3.removed[0].ends_with("lib.rs"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn sync_reparses_only_changed_file() {
        let dir = temp_dir("sync");
        std::fs::write(dir.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("lib.rs"), "pub mod module;").unwrap();
        std::fs::write(dir.join("module.rs"), "pub fn a() {}").unwrap();

        let mut scanner = WorkspaceScanner::new(dir.to_string_lossy().to_string());
        let mut engine = IncrementalGraphEngine::new();
        let mut graph = MemoryGraph::new(0.2, 100_000);

        let d0 = scanner.sync(&mut engine, &mut graph).await.unwrap();
        assert_eq!(d0.added.len(), 3);
        let mutations_after_initial = engine.total_mutations();
        assert_eq!(mutations_after_initial, 3);

        // Modify only module.rs and re-sync.
        std::fs::write(dir.join("module.rs"), "pub fn a() {}\npub fn b() {}").unwrap();
        let d1 = scanner.sync(&mut engine, &mut graph).await.unwrap();
        assert_eq!(d1.changed_count(), 1, "only one file changed");
        assert_eq!(
            engine.total_mutations() - mutations_after_initial,
            1,
            "exactly one file must be reparsed"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn watch_and_update_delta_detect_changes() {
        let dir = temp_dir("watch");
        std::fs::write(dir.join("a.rs"), "fn a() {}").unwrap();
        let mut scanner = WorkspaceScanner::new(dir.to_string_lossy().to_string());

        // `update_delta` reports the initial files.
        let d0 = scanner.update_delta().await.unwrap();
        assert_eq!(d0.added.len(), 1);

        // Modify a file; `watch_changes` must observe it within a few polls.
        std::fs::write(dir.join("a.rs"), "fn a() {}\nfn b() {}").unwrap();
        let mut changed = 0usize;
        let events = scanner
            .watch_changes(Duration::from_millis(1), 3, |d| {
                changed += d.changed_count()
            })
            .await
            .unwrap();
        assert!(events >= 1, "watch_changes must observe the modification");
        assert!(changed >= 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn removed_file_evicts_its_nodes() {
        let dir = temp_dir("evict");
        std::fs::write(dir.join("a.rs"), "pub fn a() {}\nstruct A;").unwrap();
        std::fs::write(dir.join("b.rs"), "pub fn b() {}").unwrap();

        let mut scanner = WorkspaceScanner::new(dir.to_string_lossy().to_string());
        let mut engine = IncrementalGraphEngine::new();
        let mut graph = MemoryGraph::new(0.2, 100_000);
        scanner.sync(&mut engine, &mut graph).await.unwrap();
        let before = graph.node_count();
        assert!(before > 0);

        std::fs::remove_file(dir.join("a.rs")).unwrap();
        let d = scanner.sync(&mut engine, &mut graph).await.unwrap();
        assert_eq!(d.removed.len(), 1);
        assert!(
            graph.node_count() < before,
            "removed file's nodes must be evicted"
        );
        assert_eq!(
            graph.prune_dangling_edges(),
            0,
            "no dangling edges after removal"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_rust_symlink_outside_workspace() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir("symlink_root");
        let outside = temp_dir("symlink_outside");
        let sentinel = outside.join("sentinel.rs");
        std::fs::write(&sentinel, "pub const OUTSIDE: bool = true;").unwrap();
        symlink(&sentinel, dir.join("escaped.rs")).unwrap();

        let mut scanner = WorkspaceScanner::new(dir.to_string_lossy().to_string());
        let error = scanner.scan_workspace().await.unwrap_err();
        assert!(matches!(error, WorkspaceError::UnsafePath(_)));
        assert!(
            scanner.files.is_empty(),
            "outside source must not be indexed"
        );

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[tokio::test]
    async fn rejects_oversized_rust_source_before_reading_it() {
        let dir = temp_dir("oversized");
        let file = std::fs::File::create(dir.join("large.rs")).unwrap();
        file.set_len(MAX_RUST_FILE_BYTES + 1).unwrap();

        let mut scanner = WorkspaceScanner::new(dir.to_string_lossy().to_string());
        let error = scanner.scan_workspace().await.unwrap_err();
        assert!(matches!(error, WorkspaceError::LimitExceeded(_)));
        assert!(
            scanner.files.is_empty(),
            "oversized source must not be indexed"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
