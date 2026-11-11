//! Moved to `jobbot_app::actions`. Re-exported here so every `crate::actions::...` path
//! keeps working.
//! The test left here needs the SQLite store's own error type (a database that fails), an
//! adapter only this crate wires; the other tests of `actions` are in jobbot-app's.

pub use jobbot_app::actions::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::TrackerError;

    #[test]
    fn a_disk_failure_is_explained_with_its_cause() {
        let error = TrackerError::Io {
            doing: "creating data/backups".to_string(),
            source: std::io::Error::other("disk full"),
        };
        // The SQLite adapter's error reaches the use cases as the port's backend error.
        assert_eq!(
            explain(&error.into()),
            "error de base de datos: creating data/backups: disk full"
        );
    }
}
