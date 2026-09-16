use serde::ser::{Serialize, SerializeStruct, Serializer};

/// Every fallible boundary in the app returns this. It serialises to
/// `{"code": "...", "message": "..."}` exactly as spec §8 requires.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Duplicate(String),
    #[error("{0}")]
    OutOfRange(String),
    #[error("{0}")]
    TerminalUnavailable(String),
    #[error("{0}")]
    Db(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Duplicate(_) => "duplicate",
            AppError::OutOfRange(_) => "out_of_range",
            AppError::TerminalUnavailable(_) => "terminal_unavailable",
            AppError::Db(_) => "db",
            AppError::Io(_) => "io",
            AppError::Internal(_) => "internal",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_to_code_and_message() {
        let e = AppError::NotFound("no such directory: /tmp/nope".into());
        let json = serde_json::to_value(&e).expect("serialise");
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "no such directory: /tmp/nope");
    }

    #[test]
    fn every_variant_has_a_snake_case_code() {
        let cases = vec![
            (AppError::NotFound("a".into()), "not_found"),
            (AppError::Duplicate("a".into()), "duplicate"),
            (AppError::OutOfRange("a".into()), "out_of_range"),
            (AppError::TerminalUnavailable("a".into()), "terminal_unavailable"),
            (AppError::Db("a".into()), "db"),
            (AppError::Io("a".into()), "io"),
            (AppError::Internal("a".into()), "internal"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code(), expected);
            let json = serde_json::to_value(&err).expect("serialise");
            assert_eq!(json["code"], expected);
            assert_eq!(json["message"], "a");
        }
    }

    #[test]
    fn display_is_the_message() {
        let e = AppError::OutOfRange("interval_secs must be 10..=3600".into());
        assert_eq!(e.to_string(), "interval_secs must be 10..=3600");
    }

    #[test]
    fn rusqlite_error_maps_to_db() {
        let e: AppError = rusqlite::Error::QueryReturnedNoRows.into();
        assert_eq!(e.code(), "db");
    }

    #[test]
    fn io_error_maps_to_io() {
        let e: AppError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert_eq!(e.code(), "io");
        assert_eq!(e.to_string(), "missing");
    }
}
