use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::error::{BackendError, ErrorCode};
pub use crate::infrastructure::repository_paths::is_link;
use crate::infrastructure::repository_paths::is_safe_relative;

pub fn validate_relative(path: &str) -> Result<(), BackendError> {
    if !is_safe_relative(path) {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "The conflict path is not a safe repository-relative file path.",
        ));
    }
    Ok(())
}

pub fn validate_target(root: &Path, relative: &str) -> Result<PathBuf, BackendError> {
    validate_relative(relative)?;
    let components: Vec<_> = relative.split('/').collect();
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        let last = index + 1 == components.len();
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if is_link(&metadata)
                    || (last && !metadata.is_file())
                    || (!last && !metadata.is_dir())
                {
                    return Err(BackendError::new(
                        ErrorCode::UnsupportedConflict,
                        "Symlinks, reparse points, and directory/file conflicts require external resolution.",
                    ));
                }
                if !path.canonicalize()?.starts_with(root) {
                    return Err(BackendError::new(
                        ErrorCode::InvalidPath,
                        "The path escapes the repository.",
                    ));
                }
            }
            Err(error) if last && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}
