//! Path validation functions for configuration fields.

use crate::config::Config;

/// Validate that a path points to an executable file.
/// Returns an error message if validation fails, None if valid.
pub fn validate_executable_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some("Path cannot be empty".to_string());
    }

    let expanded = Config::expand_tilde(path);

    match std::fs::metadata(&expanded) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Some("Path is not a file".to_string());
            }

            // Check executable permission on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                if mode & 0o111 == 0 {
                    return Some("File is not executable".to_string());
                }
            }

            None
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some("File not found".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Some("Cannot access file".to_string())
        }
        Err(_) => Some("Invalid path".to_string()),
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
        Ok(metadata) => {
            if !metadata.is_file() {
                Some("Path is not a file".to_string())
            } else {
                None
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some("File not found".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Some("Cannot access file".to_string())
        }
        Err(_) => Some("Invalid path".to_string()),
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
        Ok(metadata) => {
            if !metadata.is_dir() {
                Some("Path is not a directory".to_string())
            } else {
                None
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Some("Directory not found".to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Some("Cannot access directory".to_string())
        }
        Err(_) => Some("Invalid path".to_string()),
    }
}
