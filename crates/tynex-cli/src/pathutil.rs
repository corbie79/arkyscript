use std::{
    io,
    path::{Path, PathBuf},
};

pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let path = path.canonicalize()?;
    // Lua package.path treats '?' as a placeholder. Windows verbatim path
    // prefixes would accidentally become module-name substitutions.
    #[cfg(windows)]
    if let Some(text) = path.to_str() {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{rest}")));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(path)
}

/// Resolve existing ancestors as well as not-yet-saved editor files.
pub fn source_path(path: &Path) -> io::Result<PathBuf> {
    match canonicalize(path) {
        Ok(path) => Ok(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().filter(|p| *p != path).ok_or(e)?;
            let name = path
                .file_name()
                .ok_or_else(|| io::Error::other("invalid source path"))?;
            Ok(source_path(parent)?.join(name))
        }
        Err(e) => Err(e),
    }
}
