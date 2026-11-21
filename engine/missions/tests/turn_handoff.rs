//! Turn handoff between environments through a shared remote (#121): a
//! placed turn is published once however often it is asked for, a publish
//! that stopped before removing its worktree removes it when asked again, and
//! landing the run deletes what it pushed to the remote, unless it moved since.
use ahjo_mission_engine::{TurnWorkspace, TurnWorktrees, Workspaces};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).unwrap();
        let root =
            std::env::temp_dir().join(format!("ahjo-121-handoff-{}", u64::from_ne_bytes(bytes)));
        std::fs::create_dir_all(&root).unwrap();
        git(
            &root,
            &["init", "--quiet", "--bare", "-b", "main", "remote.git"],
        );
        for side in ["coordinator", "seat"] {
            git(&root, &["clone", "--quiet", "remote.git", side]);
            git(&root.join(side), &["config", "user.name", "Test"]);
            git(
                &root.join(side),
                &["config", "user.email", "test@example.invalid"],
            );
        }
        let coordinator = root.join("coordinator");
        git(
            &coordinator,
            &["commit", "--quiet", "--allow-empty", "-m", "base"],
        );
        git(&coordinator, &["push", "--quiet", "origin", "main"]);
        Self(root)
    }
    fn side(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_placed_turn_is_published_once_and_landing_deletes_its_remote_refs() {
    let scratch = Scratch::new();
    let repositories = |side: &str| BTreeMap::from([("owner/repo".to_owned(), scratch.side(side))]);
    let workspaces = Workspaces::new(scratch.0.join("runs"), repositories("coordinator"));
    let tree = workspaces.prepare("run_handoff", "owner/repo", "").unwrap();
    git(
        &tree.path,
        &[
            "push",
            "--quiet",
            "origin",
            &format!("HEAD:refs/heads/{}", tree.branch),
        ],
    );
    let turn = |attempt| TurnWorkspace {
        run_id: "run_handoff".into(),
        step_id: "fmn_port".into(),
        attempt,
        repository: "owner/repo".into(),
        commit: git(&tree.path, &["rev-parse", "HEAD"]),
    };
    let turns = TurnWorktrees::new(scratch.0.join("turns"), repositories("seat"));

    // A retry beside a publish still running waits for it and gets its answer.
    let path = turns.prepare(&turn(1)).unwrap();
    std::fs::write(path.join("port.txt"), "ported\n").unwrap();
    let (first, second) = std::thread::scope(|s| {
        let a = s.spawn(|| turns.publish(&turn(1)).unwrap());
        let b = s.spawn(|| turns.publish(&turn(1)).unwrap());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(first, second);
    assert!(path.exists());
    let remote = scratch.0.join("remote.git");
    assert_eq!(git(&remote, &["rev-parse", &first.reference]), first.commit);

    // A retry keeps its cwd, and each attempt's ref retains its own state.
    let retry_path = turns.prepare(&turn(2)).unwrap();
    assert_eq!(path, retry_path);
    assert_eq!(
        std::fs::read_to_string(path.join("port.txt")).unwrap(),
        "ported\n"
    );
    std::fs::write(path.join("port.txt"), "corrected\n").unwrap();
    let second = turns.publish(&turn(2)).unwrap();
    assert_eq!(turns.publish(&turn(2)).unwrap(), second);
    assert_eq!(
        git(&remote, &["show", &format!("{}:port.txt", first.reference)]),
        "ported"
    );
    assert_eq!(
        git(
            &remote,
            &["show", &format!("{}:port.txt", second.reference)]
        ),
        "corrected"
    );
    assert!(path.exists());
    turns.release(&turn(2)).unwrap();
    turns.release(&turn(2)).unwrap();
    assert!(!path.exists());

    let published = BTreeMap::from([
        (first.reference.clone(), first.commit.clone()),
        (second.reference.clone(), second.commit.clone()),
    ]);
    let notes = workspaces.remove(&tree, true, &published).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let left = git(&remote, &["for-each-ref", "--format=%(refname)"]);
    assert_eq!(
        left, "refs/heads/main",
        "landing leaves only main on the remote"
    );
}

#[test]
fn an_old_placed_turn_can_finish_publication_after_upgrade() {
    use sha2::{Digest, Sha256};
    let scratch = Scratch::new();
    let checkout = scratch.side("seat");
    git(&checkout, &["fetch", "--quiet", "origin", "main"]);
    let commit = git(&checkout, &["rev-parse", "origin/main"]);
    let turn = TurnWorkspace {
        run_id: "run_legacy".into(),
        step_id: "fmn_work".into(),
        attempt: 3,
        repository: "owner/repo".into(),
        commit: commit.clone(),
    };
    let root = scratch.0.join("turns");
    std::fs::create_dir_all(root.join("run_legacy")).unwrap();
    let digest = hex::encode(Sha256::digest(b"fmn_work"));
    let path = root
        .join("run_legacy")
        .join(format!("turn-{}-3", &digest[..12]));
    git(
        &checkout,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            &commit,
        ],
    );
    std::fs::write(path.join("legacy.txt"), "preserved").unwrap();
    let turns = TurnWorktrees::new(root, BTreeMap::from([("owner/repo".into(), checkout)]));
    let published = turns.publish(&turn).unwrap();
    assert!(!path.exists());
    assert_eq!(turns.publish(&turn).unwrap(), published);
    assert_eq!(
        git(
            &scratch.0.join("remote.git"),
            &["show", &format!("{}:legacy.txt", published.reference)]
        ),
        "preserved"
    );
}

#[test]
fn landing_leaves_shared_refs_that_moved_since_ahjo_pushed_them() {
    let scratch = Scratch::new();
    let checkout = scratch.side("coordinator");
    let workspaces = Workspaces::new(
        scratch.0.join("runs"),
        BTreeMap::from([("owner/repo".to_owned(), checkout.clone())]),
    );
    let tree = workspaces.prepare("run_moved", "owner/repo", "").unwrap();
    let branch = format!("refs/heads/{}", tree.branch);
    git(
        &tree.path,
        &["push", "--quiet", "origin", &format!("HEAD:{branch}")],
    );
    let pushed = git(&tree.path, &["rev-parse", "HEAD"]);
    let remote = scratch.0.join("remote.git");
    // Someone else moves the run branch and a turn ref after Ahjo pushed
    // them, and pushes a turn ref Ahjo has no record of.
    let theirs = git(
        &remote,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit-tree",
            "-p",
            &pushed,
            "-m",
            "theirs",
            &format!("{pushed}^{{tree}}"),
        ],
    );
    let turn = "refs/ahjo/turns/moved/aaaaaaaaaaaa-1";
    let stray = "refs/ahjo/turns/moved/bbbbbbbbbbbb-1";
    for reference in [branch.as_str(), turn, stray] {
        git(&remote, &["update-ref", reference, &theirs]);
    }
    let published = BTreeMap::from([(turn.to_owned(), pushed.clone())]);

    let notes = workspaces.remove(&tree, true, &published).unwrap();
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains(&branch) && n.contains(&theirs) && n.contains(&pushed))
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains(stray) && n.contains("no record"))
    );
    let mut left: Vec<String> = git(&remote, &["for-each-ref", "--format=%(refname)"])
        .lines()
        .map(str::to_owned)
        .collect();
    left.sort();
    let mut kept = ["refs/heads/main", &branch, turn, stray];
    kept.sort();
    assert_eq!(left, kept, "nothing moved is deleted");
}
