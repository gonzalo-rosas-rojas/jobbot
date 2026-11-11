//! Filesystem layout of the project, derived from one root directory: the project folder, or the
//! folder `JOBBOT_HOME` names, which then holds all private state (profile, data, CV, STOP).

use std::path::{Path, PathBuf};

/// The variable that moves all private state out of the project folder.
pub const HOME_VAR: &str = "JOBBOT_HOME";

/// Every path the tool reads or writes: all under `root`, except where a run puts Claude's
/// working folder.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub profile_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cv_master: PathBuf,
    pub stop_file: PathBuf,
    /// Where `claude -p` runs: never under the repository when the run knows `%LOCALAPPDATA%`, so
    /// no `CLAUDE.md` above it can reach the child (finding F4). When the run does not, the folder
    /// is `data/claude-cwd` and `CliClaude::locate` refuses it if an instruction file sits above.
    pub claude_work: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let data_dir = root.join("data");
        Paths {
            profile_dir: root.join("profile"),
            cv_master: root.join("cv").join("cv-master.md"),
            stop_file: root.join("STOP"),
            claude_work: data_dir.join("claude-cwd"),
            data_dir,
            root,
        }
    }

    /// The layout of a real run: `root`'s, with Claude's working folder at
    /// `%LOCALAPPDATA%\jobbot\claude-cwd` when `local_app_data` is known.
    pub fn for_run(root: impl Into<PathBuf>, local_app_data: Option<&Path>) -> Self {
        let mut paths = Paths::new(root);
        if let Some(local) = local_app_data {
            paths.claude_work = local.join("jobbot").join("claude-cwd");
        }
        paths
    }
}

/// The root of a run: the folder `JOBBOT_HOME` names when it is set (it must hold `profile/`; a
/// wrong override stops the run rather than falling back to another folder's data), otherwise
/// what `find_root` finds.
pub fn run_root(home: Option<&Path>, cwd: &Path, exe: Option<&Path>) -> Option<PathBuf> {
    match home {
        Some(home) => home.join("profile").is_dir().then(|| home.to_path_buf()),
        None => find_root(cwd, exe),
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
    fn a_run_puts_claudes_working_folder_under_local_app_data() {
        let paths = Paths::for_run("proj", Some(Path::new(r"C:\Local")));
        assert_eq!(
            paths.claude_work,
            Path::new(r"C:\Local").join("jobbot").join("claude-cwd")
        );
        assert_eq!(paths.profile_dir, PathBuf::from("proj").join("profile"));
        assert_eq!(paths.data_dir, PathBuf::from("proj").join("data"));
    }

    #[test]
    fn without_local_app_data_claudes_working_folder_stays_in_the_data_folder() {
        let in_data = PathBuf::from("proj").join("data").join("claude-cwd");
        assert_eq!(Paths::for_run("proj", None).claude_work, in_data);
        assert_eq!(Paths::new("proj").claude_work, in_data);
    }

    #[test]
    fn jobbot_home_is_the_root_of_a_run_when_it_holds_a_profile() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("profile")).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        assert_eq!(
            run_root(Some(home.path()), elsewhere.path(), None),
            Some(home.path().to_path_buf())
        );
    }

    #[test]
    fn a_jobbot_home_without_a_profile_stops_the_run_even_inside_a_project() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("profile")).unwrap();
        assert_eq!(run_root(Some(home.path()), project.path(), None), None);
    }

    #[test]
    fn without_jobbot_home_the_root_is_found_as_before() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("profile")).unwrap();
        assert_eq!(
            run_root(None, project.path(), None),
            Some(project.path().to_path_buf())
        );
    }

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
