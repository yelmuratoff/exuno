use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A refusal of `lib/helpers/backup.sh`, printed as `Error: <message>`.
    #[error("{0}")]
    Backup(String),
    #[error("Repository root not found: {}", .0.display())]
    ProjectRootNotFound(PathBuf),
    #[error("EXUNO_CONFIG_PATH is set but file not found: {}", .0.display())]
    ConfigPathNotFound(PathBuf),
    #[error("Manifest template {template}: {reason}")]
    Manifest { template: String, reason: String },
    #[error("Refusing to write {path}: {link} is a symlink to {target}, outside the project")]
    EscapingOutput {
        path: String,
        link: String,
        target: String,
    },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn is_broken_pipe(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == std::io::ErrorKind::BrokenPipe)
    }
}
