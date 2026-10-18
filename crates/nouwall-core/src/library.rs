//! Wallpaper library on disk: one folder per wallpaper holding `wallpaper.toml`
//! and its assets.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::manifest::{slugify, Manifest};

pub const MANIFEST_FILE: &str = "wallpaper.toml";

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Entry {
    /// Absolute path of the entry file (meaningless for remote web wallpapers).
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.entry)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("{path}: {source}")]
    Manifest { path: PathBuf, source: toml::de::Error },
    #[error("wallpaper \"{0}\" tidak ditemukan")]
    NotFound(String),
    #[error("id \"{0}\" tidak valid")]
    InvalidId(String),
}

#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All readable wallpapers sorted by title. Broken folders are reported separately.
    pub fn scan(&self) -> (Vec<Entry>, Vec<LibraryError>) {
        let mut entries = Vec::new();
        let mut errors = Vec::new();
        let dirs = match fs::read_dir(&self.root) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return (entries, errors),
            Err(e) => return (entries, vec![e.into()]),
        };
        for dir in dirs.flatten() {
            if !dir.path().is_dir() {
                continue;
            }
            let id = dir.file_name().to_string_lossy().into_owned();
            match self.get(&id) {
                Ok(e) => entries.push(e),
                Err(LibraryError::NotFound(_)) => {}
                Err(e) => errors.push(e),
            }
        }
        entries.sort_by(|a, b| a.manifest.title.to_lowercase().cmp(&b.manifest.title.to_lowercase()));
        (entries, errors)
    }

    pub fn get(&self, id: &str) -> Result<Entry, LibraryError> {
        validate_id(id)?;
        let dir = self.root.join(id);
        let path = dir.join(MANIFEST_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(LibraryError::NotFound(id.into())),
            Err(e) => return Err(e.into()),
        };
        let manifest = Manifest::parse(&text).map_err(|source| LibraryError::Manifest { path, source })?;
        Ok(Entry { id: id.into(), dir, manifest })
    }

    /// A free id derived from `name` (adds `-2`, `-3`, ... on collision).
    pub fn free_id(&self, name: &str) -> String {
        let base = slugify(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.root.join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Create a wallpaper folder, write the manifest and return the entry.
    /// `files` are `(name inside folder, bytes)`.
    pub fn create(&self, id: &str, manifest: &Manifest, files: &[(&str, &[u8])]) -> Result<Entry, LibraryError> {
        validate_id(id)?;
        let dir = self.root.join(id);
        fs::create_dir_all(&dir)?;
        for (name, bytes) in files {
            fs::write(dir.join(name), bytes)?;
        }
        fs::write(dir.join(MANIFEST_FILE), manifest.to_toml())?;
        self.get(id)
    }

    /// Import a single file by copying it into a new folder.
    pub fn import_file(&self, source: &Path, mut manifest: Manifest) -> Result<Entry, LibraryError> {
        let file_name = source
            .file_name()
            .ok_or_else(|| LibraryError::InvalidId(source.display().to_string()))?
            .to_string_lossy()
            .into_owned();
        let id = self.free_id(&manifest.title);
        let dir = self.root.join(&id);
        fs::create_dir_all(&dir)?;
        fs::copy(source, dir.join(&file_name))?;
        manifest.entry = file_name;
        fs::write(dir.join(MANIFEST_FILE), manifest.to_toml())?;
        self.get(&id)
    }

    /// Import a folder (e.g. an HTML wallpaper with assets) by copying it recursively.
    pub fn import_dir(&self, source: &Path, manifest: Manifest) -> Result<Entry, LibraryError> {
        let id = self.free_id(&manifest.title);
        let dir = self.root.join(&id);
        copy_dir(source, &dir)?;
        fs::write(dir.join(MANIFEST_FILE), manifest.to_toml())?;
        self.get(&id)
    }

    pub fn remove(&self, id: &str) -> Result<(), LibraryError> {
        validate_id(id)?;
        let dir = self.root.join(id);
        if !dir.join(MANIFEST_FILE).exists() {
            return Err(LibraryError::NotFound(id.into()));
        }
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    pub fn update_manifest(&self, id: &str, manifest: &Manifest) -> Result<(), LibraryError> {
        validate_id(id)?;
        fs::write(self.root.join(id).join(MANIFEST_FILE), manifest.to_toml())?;
        Ok(())
    }
}

fn validate_id(id: &str) -> Result<(), LibraryError> {
    let ok = !id.is_empty()
        && id.len() <= 96
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !id.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(LibraryError::InvalidId(id.into()))
    }
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Kind;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("nouwall-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    fn shader(title: &str) -> Manifest {
        Manifest {
            title: title.into(),
            kind: Kind::Shader,
            entry: "main.glsl".into(),
            author: String::new(),
            description: String::new(),
            video: None,
            shader_cost: None,
        }
    }

    #[test]
    fn create_scan_remove() {
        let root = tmp("lib");
        let lib = Library::new(&root);
        assert!(lib.scan().0.is_empty());
        lib.create("b-waves", &shader("Waves"), &[("main.glsl", b"void mainImage(){}")]).unwrap();
        lib.create("a-aurora", &shader("aurora"), &[("main.glsl", b"")]).unwrap();
        fs::create_dir_all(root.join("broken")).unwrap();
        fs::write(root.join("broken").join(MANIFEST_FILE), "kind = 3").unwrap();
        let (entries, errors) = lib.scan();
        assert_eq!(entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a-aurora", "b-waves"]);
        assert_eq!(errors.len(), 1);
        assert_eq!(lib.free_id("Waves"), "waves");
        lib.remove("b-waves").unwrap();
        assert!(matches!(lib.get("b-waves"), Err(LibraryError::NotFound(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_path_traversal() {
        let lib = Library::new(tmp("trav"));
        assert!(matches!(lib.get("../etc"), Err(LibraryError::InvalidId(_))));
        assert!(matches!(lib.remove(".."), Err(LibraryError::InvalidId(_))));
    }

    #[test]
    fn import_file_copies_and_dedupes_id() {
        let root = tmp("imp");
        let src = tmp("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("clip.mp4");
        fs::write(&file, b"data").unwrap();
        let lib = Library::new(&root);
        let mut m = shader("Clip");
        m.kind = Kind::Video;
        let a = lib.import_file(&file, m.clone()).unwrap();
        let b = lib.import_file(&file, m).unwrap();
        assert_eq!((a.id.as_str(), b.id.as_str()), ("clip", "clip-2"));
        assert_eq!(fs::read(b.entry_path()).unwrap(), b"data");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(src).unwrap();
    }
}
