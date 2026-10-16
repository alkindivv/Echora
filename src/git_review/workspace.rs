//! Composer checkout operations. Git owns conflict detection; never force a switch.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::{
    Checkout, checked, checkout, command, git, output, process, resolve, safe_path, status,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branches {
    pub current: Checkout,
    pub names: Vec<String>,
    pub dirty_files: usize,
    pub default_remote: Option<String>,
}

pub fn branches(cwd: &Path) -> Result<Branches> {
    let current = checkout(cwd);
    if !current.is_repository() {
        bail!(
            crate::i18n::format!("此目录不是 Git 仓库" => "This directory is not a Git repository")
        );
    }
    // No count limit: searching and scrolling must reach every local branch.
    let refs = git(
        cwd,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:lstrip=2)",
            "refs/heads",
        ],
    )?;
    let mut names: Vec<String> = refs.lines().map(str::to_owned).collect();
    let remotes = git(
        cwd,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:lstrip=2)%09%(symref)",
            "refs/remotes",
        ],
    )?;
    for line in remotes.lines() {
        let Some((remote, symbolic)) = line.split_once('\t') else {
            continue;
        };
        let Some((_, local)) = remote.split_once('/') else {
            continue;
        };
        if symbolic.is_empty() && !names.iter().any(|name| name == local || name == remote) {
            names.push(remote.to_owned());
        }
    }
    if let Some(name) = current.branch() {
        names.retain(|value| value != name);
        names.insert(0, name.to_owned());
    }
    let default_remote = git(
        cwd,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .map(|name| name.trim().to_owned())
    .or_else(|| {
        git(cwd, &["show-ref", "--verify", "refs/remotes/origin/main"])
            .ok()
            .map(|_| "origin/main".to_owned())
    });
    Ok(Branches {
        current,
        names,
        dirty_files: status(cwd)?.len(),
        default_remote,
    })
}

pub fn switch(cwd: &Path, branch: &str, create: bool) -> Result<Checkout> {
    git(cwd, &["check-ref-format", "--branch", branch])?;
    let mut expected = branch;
    if create {
        git(cwd, &["switch", "-c", branch])?;
    } else {
        if git(
            cwd,
            &["show-ref", "--verify", &format!("refs/heads/{branch}")],
        )
        .is_ok()
        {
            git(cwd, &["switch", "--no-guess", branch])?;
        } else {
            git(
                cwd,
                &["show-ref", "--verify", &format!("refs/remotes/{branch}")],
            )?;
            let (_, local) = branch.split_once('/').context("Invalid remote branch")?;
            git(cwd, &["switch", "--track", "-c", local, branch])?;
            expected = local;
        }
    }
    let actual = checkout(cwd);
    if actual.branch() != Some(expected) {
        bail!(
            crate::i18n::format!("Git 未切换到所选分支" => "Git did not switch to the selected branch")
        );
    }
    Ok(actual)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub cwd: PathBuf,
    pub local_cwd: PathBuf,
}

pub fn create_worktree(
    cwd: &Path,
    base: &str,
    include_changes: bool,
    worktrees_root: &Path,
) -> Result<Worktree> {
    let root = PathBuf::from(git(cwd, &["rev-parse", "--show-toplevel"])?.trim());
    // Resolve before allocating a directory; keep the chosen commit stable.
    let commit = resolve(cwd, base)?;
    let root = std::fs::canonicalize(root)?;
    let local_cwd = std::fs::canonicalize(cwd)?;
    let relative = local_cwd
        .strip_prefix(&root)
        .context("Working directory is outside repository")?
        .to_owned();
    let changes = include_changes
        .then(|| LocalChanges::read(&root, &commit))
        .transpose()?;
    std::fs::create_dir_all(worktrees_root)?;
    let mut allocation = None;
    for sequence in 0..1000 {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let candidate = worktrees_root.join(format!("{stamp:x}-{}-{sequence}", std::process::id()));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                allocation = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let allocation = allocation.context("Unable to allocate a worktree directory")?;
    let destination = allocation.join(
        root.file_name()
            .context("Repository has no directory name")?,
    );
    let result = (|| {
        git(
            &root,
            &[
                "worktree",
                "add",
                "--detach",
                &destination.to_string_lossy(),
                &commit,
            ],
        )?;
        if let Some(changes) = &changes {
            changes.apply(&root, &destination)?;
        }
        let target = destination.join(relative);
        if !target.is_dir() {
            bail!(
                crate::i18n::format!("所选分支中不存在当前工作目录" => "The selected branch does not contain the current working directory")
            );
        }
        Ok(Worktree {
            cwd: target,
            local_cwd,
        })
    })();
    if result.is_err() {
        // Only remove this operation's fresh, unused checkout on failure.
        let _ = git(
            &root,
            &[
                "worktree",
                "remove",
                "--force",
                &destination.to_string_lossy(),
            ],
        );
        let _ = std::fs::remove_dir(&allocation);
    }
    result
}

struct LocalChanges {
    staged: Vec<u8>,
    unstaged: Vec<u8>,
    untracked: Vec<PathBuf>,
}

impl LocalChanges {
    fn read(root: &Path, commit: &str) -> Result<Self> {
        if resolve(root, "HEAD")? != commit {
            bail!(
                crate::i18n::format!("本地修改只能应用到当前检出的提交" => "Local changes can only be copied from the current checkout")
            );
        }
        let staged = checked(output(
            root,
            &[
                "diff",
                "--cached",
                "--binary",
                "--no-ext-diff",
                "--no-textconv",
                "HEAD",
                "--",
            ],
        )?)?;
        let unstaged = checked(output(
            root,
            &["diff", "--binary", "--no-ext-diff", "--no-textconv", "--"],
        )?)?;
        let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| safe_path(path).map(Path::to_owned))
            .collect::<Result<_>>()?;
        Ok(Self {
            staged,
            unstaged,
            untracked,
        })
    }

    fn apply(&self, source: &Path, destination: &Path) -> Result<()> {
        for (patch, staged) in [(&self.staged, true), (&self.unstaged, false)] {
            if patch.is_empty() {
                continue;
            }
            let mut cmd = command(destination);
            cmd.args(["apply", "--binary"]);
            if staged {
                cmd.arg("--index");
            }
            checked(process::run(
                &mut cmd,
                Some(patch),
                std::time::Duration::from_secs(15),
            )?)?;
        }
        for path in &self.untracked {
            let target = destination.join(path);
            let mut parent = destination.to_owned();
            for component in path.parent().into_iter().flat_map(Path::components) {
                parent.push(component);
                if std::fs::symlink_metadata(&parent).is_ok_and(|m| m.file_type().is_symlink()) {
                    bail!(
                        "Untracked file parent is a symbolic link: {}",
                        path.display()
                    );
                }
                std::fs::create_dir_all(&parent)?;
            }
            if target.symlink_metadata().is_ok() {
                bail!("Worktree file already exists: {}", path.display());
            }
            let source = source.join(path);
            #[cfg(unix)]
            if source.symlink_metadata()?.file_type().is_symlink() {
                std::os::unix::fs::symlink(std::fs::read_link(source)?, target)?;
                continue;
            }
            std::fs::copy(source, target)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
