use std::path::{Component, Path, PathBuf};

/// Lexically normalizes a path relative to the notebook root: drops `.` components and resolves `..`,
/// so that `./journal/a.md`, `journal/a.md` and `journal/x/../a.md` are all `journal/a.md`.
/// Leading `..` that would leave the root are kept, such paths never match a note.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(normalized.components().next_back(), Some(Component::Normal(_))) => {
                normalized.pop();
            }
            component => normalized.push(component),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_relative_paths() {
        assert_eq!(normalize(Path::new("./journal/a.md")), Path::new("journal/a.md"));
        assert_eq!(normalize(Path::new("journal/x/../a.md")), Path::new("journal/a.md"));
        assert_eq!(normalize(Path::new("journal/..")), Path::new(""));
        assert_eq!(normalize(Path::new("../a.md")), Path::new("../a.md"));
        assert_eq!(normalize(Path::new("a/../../b.md")), Path::new("../b.md"));
    }
}
