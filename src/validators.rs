//! Path validation functions for configuration fields.

use crate::config::Config;

/// Check if metadata indicates a valid executable file (pure function).
/// Returns an error message if validation fails, None if valid.
///
/// Parameters:
/// - `is_file`: whether the path is a file
/// - `mode`: Unix permission mode (ignored on non-Unix platforms)
#[allow(unused_variables)]
fn check_executable_metadata(is_file: bool, mode: u32) -> Option<String> {
    if !is_file {
        return Some("Path is not a file".to_string());
    }

    #[cfg(unix)]
    {
        if mode & 0o111 == 0 {
            return Some("File is not executable".to_string());
        }
    }

    None
}

/// Convert an I/O error to an appropriate error message for file validation.
fn file_error_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "File not found".to_string(),
        std::io::ErrorKind::PermissionDenied => "Cannot access file".to_string(),
        _ => "Invalid path".to_string(),
    }
}

/// Validate that a path points to an executable file.
/// Returns an error message if validation fails, None if valid.
pub fn validate_executable_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some("Path cannot be empty".to_string());
    }

    let expanded = Config::expand_tilde(path);

    match std::fs::metadata(&expanded) {
        Ok(metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                check_executable_metadata(metadata.is_file(), metadata.permissions().mode())
            }
            #[cfg(not(unix))]
            {
                check_executable_metadata(metadata.is_file(), 0)
            }
        }
        Err(e) => Some(file_error_message(&e)),
    }
}

/// Check if metadata indicates a valid file (pure function).
/// Returns an error message if validation fails, None if valid.
fn check_file_metadata(is_file: bool) -> Option<String> {
    if !is_file {
        Some("Path is not a file".to_string())
    } else {
        None
    }
}

/// Validate that a path points to an existing file.
/// Returns an error message if validation fails, None if valid.
pub fn validate_file_exists(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some("Path cannot be empty".to_string());
    }

    let expanded = Config::expand_tilde(path);

    match std::fs::metadata(&expanded) {
        Ok(metadata) => check_file_metadata(metadata.is_file()),
        Err(e) => Some(file_error_message(&e)),
    }
}

/// Check if metadata indicates a valid directory (pure function).
/// Returns an error message if validation fails, None if valid.
fn check_directory_metadata(is_dir: bool) -> Option<String> {
    if !is_dir {
        Some("Path is not a directory".to_string())
    } else {
        None
    }
}

/// Convert an I/O error to an appropriate error message for directory validation.
fn directory_error_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "Directory not found".to_string(),
        std::io::ErrorKind::PermissionDenied => "Cannot access directory".to_string(),
        _ => "Invalid path".to_string(),
    }
}

/// Validate that a path points to an existing directory.
/// Returns an error message if validation fails, None if valid.
pub fn validate_directory_exists(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some("Path cannot be empty".to_string());
    }

    let expanded = Config::expand_tilde(path);

    match std::fs::metadata(&expanded) {
        Ok(metadata) => check_directory_metadata(metadata.is_dir()),
        Err(e) => Some(directory_error_message(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for check_executable_metadata (pure function)

    #[test]
    fn test_check_executable_metadata_not_a_file() {
        // A directory (is_file = false) should return an error
        let result = check_executable_metadata(false, 0o755);
        assert_eq!(result, Some("Path is not a file".to_string()));
    }

    #[test]
    fn test_check_executable_metadata_file_not_executable() {
        // A file without execute bits should return an error on Unix
        let result = check_executable_metadata(true, 0o644);
        #[cfg(unix)]
        assert_eq!(result, Some("File is not executable".to_string()));
        #[cfg(not(unix))]
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_executable_metadata_user_executable() {
        // A file with user execute bit should be valid
        let result = check_executable_metadata(true, 0o755);
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_executable_metadata_group_executable() {
        // A file with only group execute bit should be valid
        let result = check_executable_metadata(true, 0o010);
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_executable_metadata_other_executable() {
        // A file with only other execute bit should be valid
        let result = check_executable_metadata(true, 0o001);
        assert_eq!(result, None);
    }

    // Tests for check_file_metadata (pure function)

    #[test]
    fn test_check_file_metadata_valid_file() {
        let result = check_file_metadata(true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_file_metadata_not_a_file() {
        let result = check_file_metadata(false);
        assert_eq!(result, Some("Path is not a file".to_string()));
    }

    // Tests for check_directory_metadata (pure function)

    #[test]
    fn test_check_directory_metadata_valid_directory() {
        let result = check_directory_metadata(true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_check_directory_metadata_not_a_directory() {
        let result = check_directory_metadata(false);
        assert_eq!(result, Some("Path is not a directory".to_string()));
    }

    // Tests for file_error_message (pure function)

    #[test]
    fn test_file_error_message_not_found() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        assert_eq!(file_error_message(&error), "File not found");
    }

    #[test]
    fn test_file_error_message_permission_denied() {
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(file_error_message(&error), "Cannot access file");
    }

    #[test]
    fn test_file_error_message_other_error() {
        let error = std::io::Error::new(std::io::ErrorKind::Other, "other");
        assert_eq!(file_error_message(&error), "Invalid path");
    }

    // Tests for directory_error_message (pure function)

    #[test]
    fn test_directory_error_message_not_found() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        assert_eq!(directory_error_message(&error), "Directory not found");
    }

    #[test]
    fn test_directory_error_message_permission_denied() {
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(directory_error_message(&error), "Cannot access directory");
    }

    #[test]
    fn test_directory_error_message_other_error() {
        let error = std::io::Error::new(std::io::ErrorKind::Other, "other");
        assert_eq!(directory_error_message(&error), "Invalid path");
    }
}
