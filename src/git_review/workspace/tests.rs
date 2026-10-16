use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "echora-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-b", "main"]).unwrap();
        git(&path, &["config", "user.name", "Workspace Test"]).unwrap();
        git(
            &path,
            &["config", "user.email", "workspace@example.invalid"],
        )
        .unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &[u8]) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn commit(&self) {
        git(&self.0, &["add", "--all"]).unwrap();
        git(&self.0, &["commit", "-m", "fixture"]).unwrap();
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lists_every_branch_without_the_review_menus_count_limit() {
    let repo = Repo::new();
    repo.write("file", b"main\n");
    repo.commit();
    for index in 0..125 {
        git(&repo.0, &["branch", &format!("topic/{index:03}")]).unwrap();
    }
    let list = branches(&repo.0).unwrap();
    assert_eq!(list.names.len(), 126);
    assert_eq!(list.names[0], "main");
    assert!(list.names.contains(&"topic/124".to_owned()));
}

#[test]
fn switches_real_files_and_keeps_dirty_and_in_use_checkouts_intact() {
    let repo = Repo::new();
    repo.write("file", b"main\n");
    repo.commit();
    switch(&repo.0, "topic", true).unwrap();
    repo.write("file", b"topic\n");
    repo.commit();
    switch(&repo.0, "main", false).unwrap();
    assert_eq!(std::fs::read(repo.0.join("file")).unwrap(), b"main\n");
    repo.write("file", b"unsaved\n");
    assert!(switch(&repo.0, "topic", false).is_err());
    assert_eq!(checkout(&repo.0), Checkout::Branch("main".into()));
    assert_eq!(std::fs::read(repo.0.join("file")).unwrap(), b"unsaved\n");
    assert!(switch(&repo.0, "--detach", false).is_err());
    assert!(switch(&repo.0, "missing", false).is_err());
    repo.write("file", b"main\n");
    git(
        &repo.0,
        &[
            "worktree",
            "add",
            &repo.0.join("other").to_string_lossy(),
            "topic",
        ],
    )
    .unwrap();
    assert!(switch(&repo.0, "topic", false).is_err());
    assert_eq!(checkout(&repo.0), Checkout::Branch("main".into()));
}

#[test]
fn remote_only_branches_are_listed_and_become_real_tracking_branches() {
    let repo = Repo::new();
    repo.write("file", b"main\n");
    repo.commit();
    git(
        &repo.0,
        &["update-ref", "refs/remotes/origin/remote-only", "HEAD"],
    )
    .unwrap();
    git(&repo.0, &["config", "remote.origin.url", "."]).unwrap();
    git(
        &repo.0,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    )
    .unwrap();
    assert!(
        branches(&repo.0)
            .unwrap()
            .names
            .contains(&"origin/remote-only".into())
    );
    assert_eq!(
        switch(&repo.0, "origin/remote-only", false).unwrap(),
        Checkout::Branch("remote-only".into())
    );
    assert_eq!(
        git(&repo.0, &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .unwrap()
            .trim(),
        "origin/remote-only"
    );
}

#[test]
fn worktree_uses_selected_commit_and_preserves_source_changes_and_subdirectory() {
    let repo = Repo::new();
    repo.write("sub/file", b"base\n");
    repo.commit();
    git(&repo.0, &["branch", "base"]).unwrap();
    repo.write("sub/file", b"main\n");
    repo.commit();
    repo.write("sub/file", b"unsaved\n");
    let worktree = create_worktree(
        &repo.0.join("sub"),
        "base",
        false,
        &repo.0.join("worktrees"),
    )
    .unwrap();
    assert_eq!(checkout(&worktree.cwd), Checkout::Detached);
    assert_eq!(std::fs::read(worktree.cwd.join("file")).unwrap(), b"base\n");
    assert_eq!(
        std::fs::read(repo.0.join("sub/file")).unwrap(),
        b"unsaved\n"
    );
    assert_eq!(checkout(&repo.0), Checkout::Branch("main".into()));
    assert!(create_worktree(&repo.0, "missing", false, &repo.0.join("worktrees")).is_err());
}

#[test]
fn worktree_can_copy_staged_unstaged_binary_and_untracked_local_changes() {
    let repo = Repo::new();
    repo.write("file", b"base\n");
    repo.write("binary", &[0, 1, 2, 3]);
    repo.commit();
    repo.write("file", b"staged\n");
    git(&repo.0, &["add", "file"]).unwrap();
    repo.write("file", b"unstaged\n");
    repo.write("binary", &[0, 3, 2, 1]);
    repo.write("untracked/a file\n.txt", b"new\n");
    let before = git(&repo.0, &["status", "--porcelain=v1"]).unwrap();
    let worktree = create_worktree(&repo.0, "main", true, &repo.0.join("worktrees")).unwrap();
    assert_eq!(
        std::fs::read(worktree.cwd.join("file")).unwrap(),
        b"unstaged\n"
    );
    assert_eq!(git(&worktree.cwd, &["show", ":file"]).unwrap(), "staged\n");
    assert_eq!(
        std::fs::read(worktree.cwd.join("binary")).unwrap(),
        [0, 3, 2, 1]
    );
    assert_eq!(
        std::fs::read(worktree.cwd.join("untracked/a file\n.txt")).unwrap(),
        b"new\n"
    );
    assert_eq!(
        git(
            &repo.0,
            &[
                "status",
                "--porcelain=v1",
                "--",
                "file",
                "binary",
                "untracked"
            ]
        )
        .unwrap(),
        before
    );
    assert_eq!(checkout(&repo.0), Checkout::Branch("main".into()));
}
