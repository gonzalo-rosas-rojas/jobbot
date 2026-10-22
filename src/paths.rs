//! Filesystem layout of the project, derived from one root directory.

use std::path::{Path, PathBuf};

/// Every path the tool reads or writes, all under `root`.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub profile_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cv_master: PathBuf,
    pub stop_file: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Paths {
            profile_dir: root.join("profile"),
            data_dir: root.join("data"),
            cv_master: root.join("cv").join("cv-master.md"),
            stop_file: root.join("STOP"),
            root,
        }
    }
}

/// The project root for a run started in `cwd` by the executable at `exe`: `cwd` when it holds
/// the `profile` folder, otherwise the nearest folder above `exe` that does (the binary lives in
/// `<root>/target/release`). `None` when neither is a project, so a run from the wrong folder
/// stops instead of opening a fresh database and a STOP file nobody reads.
pub fn find_root(cwd: &Path, exe: Option<&Path>) -> Option<PathBuf> {
    let is_project = |dir: &Path| dir.join("profile").is_dir();
    if is_project(cwd) {
        return Some(cwd.to_path_buf());
    }
    exe?.ancestors()
        .skip(1)
        .find(|dir| is_project(dir))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_run_from_inside_the_project_uses_the_current_folder() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("profile")).unwrap();

        assert_eq!(
            find_root(project.path(), None),
            Some(project.path().to_path_buf())
        );
    }

    #[test]
    fn a_run_from_another_folder_finds_the_project_from_the_executable() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("profile")).unwrap();
        let release = project.path().join("target").join("release");
        std::fs::create_dir_all(&release).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();

        assert_eq!(
            find_root(elsewhere.path(), Some(&release.join("jobbot.exe"))),
            Some(project.path().to_path_buf())
        );
    }

    #[test]
    fn a_folder_that_is_not_a_project_is_not_a_root() {
        let elsewhere = tempfile::tempdir().unwrap();
        let exe_home = tempfile::tempdir().unwrap();

        assert_eq!(
            find_root(elsewhere.path(), Some(&exe_home.path().join("jobbot.exe"))),
            None
        );
        assert_eq!(find_root(elsewhere.path(), None), None);
    }

    #[test]
    fn paths_are_derived_from_the_root() {
        let p = Paths::new("proj");
        assert_eq!(p.root, PathBuf::from("proj"));
        assert_eq!(p.profile_dir, PathBuf::from("proj").join("profile"));
        assert_eq!(p.data_dir, PathBuf::from("proj").join("data"));
        assert_eq!(
            p.cv_master,
            PathBuf::from("proj").join("cv").join("cv-master.md")
        );
        assert_eq!(p.stop_file, PathBuf::from("proj").join("STOP"));
    }
}
