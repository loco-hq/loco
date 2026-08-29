//! The bundle: a version's static file tree, uploaded as a zip.
//!
//! `PUT /schema/{account}/{project}/{version}/bundle` carries a zip of a Vite
//! `dist/` — `index.html` at the zip root — and replaces the whole tree. This
//! module is the pure half of that: unpacking the archive into a
//! [`FileTree`] under fixed limits, and describing a stored tree
//! (`{ hash, uploaded_at, size }`) without handing back its bytes.
//!
//! Nothing here touches the filesystem; the store's adapter does the writing.

use std::io::Read;

use loco_schema_runtime::FileTree;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Largest request body a bundle PUT accepts (compressed bytes on the wire).
/// A Vite `dist/` for an app of the size Loco hosts today is a few hundred KB;
/// 32 MiB leaves a wide margin for images shipped with the UI while keeping a
/// single upload something the process can hold in memory.
pub const MAX_ZIP_BYTES: usize = 32 * 1024 * 1024;

/// Largest tree, summed over every file, after decompression. Bounds the zip
/// bomb an attacker can spend `MAX_ZIP_BYTES` on.
pub const MAX_TREE_BYTES: u64 = 64 * 1024 * 1024;

/// Largest single file in the tree.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Most files one tree may hold. Hashed Vite output is dozens of files; 2000
/// covers an app that ships an icon set and still bounds the inode churn of a
/// whole-tree replace.
pub const MAX_FILES: usize = 2_000;

/// The file every bundle must have at the zip root — what the site serves at
/// `/`, and the SPA fallback target once serving lands (#30).
pub const ENTRY_FILE: &str = "index.html";

/// Why a zip was refused. Every variant is the caller's fault: a bundle PUT
/// answers 400 with the message.
#[derive(Debug)]
pub enum BundleError {
    /// The body is not a readable zip archive.
    NotAZip(String),
    /// An entry could not be decompressed.
    Unreadable { path: String, reason: String },
    /// An entry name is not a safe relative path (`..`, absolute, empty
    /// segment, backslash, NUL).
    UnsafePath(String),
    /// An entry is a symlink. The tree is opaque bytes; a link in it points
    /// somewhere the tree does not own.
    Symlink(String),
    /// A limit in this module was exceeded.
    TooLarge(String),
    /// No `index.html` at the zip root.
    MissingEntryFile,
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAZip(reason) => write!(f, "bundle is not a readable zip archive: {reason}"),
            Self::Unreadable { path, reason } => {
                write!(f, "could not read {path} out of the zip: {reason}")
            }
            Self::UnsafePath(path) => write!(f, "unsafe path in bundle zip: {path}"),
            Self::Symlink(path) => write!(f, "symlink in bundle zip: {path}"),
            Self::TooLarge(what) => write!(f, "bundle too large: {what}"),
            Self::MissingEntryFile => {
                write!(f, "bundle zip has no {ENTRY_FILE} at its root")
            }
        }
    }
}

impl std::error::Error for BundleError {}

/// Unpack a zip into a file tree, or refuse it.
///
/// Directory entries are dropped (the tree is files; parents are implied).
/// Everything else must be a plain file with a safe relative name, and the
/// archive as a whole must stay under the limits above and carry an
/// [`ENTRY_FILE`] at its root.
pub fn unpack_zip(bytes: &[u8]) -> Result<FileTree, BundleError> {
    if bytes.len() > MAX_ZIP_BYTES {
        return Err(BundleError::TooLarge(format!(
            "{} compressed bytes, limit is {MAX_ZIP_BYTES}",
            bytes.len()
        )));
    }

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| BundleError::NotAZip(e.to_string()))?;

    let mut tree = FileTree::new();
    let mut total: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| BundleError::NotAZip(e.to_string()))?;
        let name = entry.name().to_string();

        if entry.is_dir() {
            continue;
        }
        // Unix mode is advisory metadata in the archive, so this is a check on
        // what the zip *claims*: a link is never staged, and the adapter
        // refuses to follow one on disk regardless.
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(BundleError::Symlink(name));
            }
        }
        if tree.len() >= MAX_FILES {
            return Err(BundleError::TooLarge(format!(
                "more than {MAX_FILES} files"
            )));
        }

        // The declared size is a hint an attacker controls; cap the reader
        // instead of trusting it, one byte past the limit so an overrun is
        // visible.
        let mut buf = Vec::new();
        let room = MAX_FILE_BYTES.min(MAX_TREE_BYTES - total) + 1;
        entry
            .by_ref()
            .take(room)
            .read_to_end(&mut buf)
            .map_err(|e| BundleError::Unreadable {
                path: name.clone(),
                reason: e.to_string(),
            })?;
        let len = buf.len() as u64;
        if len >= room {
            return Err(BundleError::TooLarge(format!(
                "{name} exceeds the {MAX_FILE_BYTES}-byte file limit or the \
                 {MAX_TREE_BYTES}-byte tree limit"
            )));
        }
        total += len;

        // `insert` is what rejects `..`, absolute paths, and empty segments —
        // the same rule the persistence layer enforces on disk.
        tree.insert(&name, buf)
            .map_err(|_| BundleError::UnsafePath(name.clone()))?;
    }

    if !tree.contains(ENTRY_FILE) {
        return Err(BundleError::MissingEntryFile);
    }

    Ok(tree)
}

/// What a GET on a bundle reports: enough to tell two uploads apart and to
/// see how big the tree is, without shipping the files.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BundleMetadata {
    /// SHA-256 over the tree's contents (see [`hash_tree`]).
    pub hash: String,
    /// RFC 3339, UTC. When the current tree was written.
    pub uploaded_at: String,
    /// Total bytes across every file.
    pub size: u64,
    /// How many files the tree holds.
    pub files: usize,
}

impl BundleMetadata {
    pub fn new(tree: &FileTree, uploaded_at: std::time::SystemTime) -> Self {
        Self {
            hash: hash_tree(tree),
            uploaded_at: chrono::DateTime::<chrono::Utc>::from(uploaded_at).to_rfc3339(),
            size: tree.total_bytes() as u64,
            files: tree.len(),
        }
    }
}

/// A content hash of the whole tree: SHA-256 over every `(path, bytes)` pair
/// in sorted order, length-prefixed so `a/b` + `c` cannot collide with `a` +
/// `b/c`. Two uploads with the same files hash the same regardless of zip
/// ordering, compression, or timestamps.
pub fn hash_tree(tree: &FileTree) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in tree.iter() {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn unpacks_a_dist_zip() {
        let bytes = zip_of(&[
            ("index.html", b"<!doctype html>"),
            ("assets/index-abc123.js", b"console.log(1)"),
        ]);
        let tree = unpack_zip(&bytes).unwrap();
        assert_eq!(tree.paths(), vec!["assets/index-abc123.js", "index.html"]);
        assert_eq!(tree.get("index.html").unwrap(), b"<!doctype html>");
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in ["../escape.html", "/etc/passwd", "assets/../../escape.js"] {
            let bytes = zip_of(&[("index.html", b"hi"), (bad, b"x")]);
            assert!(
                matches!(unpack_zip(&bytes), Err(BundleError::UnsafePath(_))),
                "expected {bad:?} to be refused"
            );
        }
    }

    #[test]
    fn rejects_a_symlink_entry() {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file("index.html", SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"hi").unwrap();
        w.add_symlink("link.js", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        let bytes = w.finish().unwrap().into_inner();

        assert!(matches!(unpack_zip(&bytes), Err(BundleError::Symlink(_))));
    }

    #[test]
    fn rejects_a_zip_without_index_html() {
        let bytes = zip_of(&[("assets/index-abc123.js", b"console.log(1)")]);
        assert!(matches!(
            unpack_zip(&bytes),
            Err(BundleError::MissingEntryFile)
        ));
        // Nested is not the root.
        let bytes = zip_of(&[("dist/index.html", b"hi")]);
        assert!(matches!(
            unpack_zip(&bytes),
            Err(BundleError::MissingEntryFile)
        ));
    }

    #[test]
    fn rejects_a_body_that_is_not_a_zip() {
        assert!(matches!(
            unpack_zip(b"not a zip at all"),
            Err(BundleError::NotAZip(_))
        ));
    }

    #[test]
    fn rejects_a_file_over_the_file_limit() {
        // Stored, not deflated: the point is the *unpacked* size, and this
        // keeps the test from spending a second compressing 16 MiB of 'a'.
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("index.html", stored).unwrap();
        w.write_all(b"hi").unwrap();
        w.start_file("big.bin", stored).unwrap();
        w.write_all(&vec![b'a'; (MAX_FILE_BYTES + 1) as usize])
            .unwrap();
        let bytes = w.finish().unwrap().into_inner();

        assert!(matches!(unpack_zip(&bytes), Err(BundleError::TooLarge(_))));
    }

    #[test]
    fn hash_is_content_addressed_not_order_dependent() {
        let a = unpack_zip(&zip_of(&[
            ("index.html", b"hi"),
            ("assets/app.js", b"console.log(1)"),
        ]))
        .unwrap();
        let b = unpack_zip(&zip_of(&[
            ("assets/app.js", b"console.log(1)"),
            ("index.html", b"hi"),
        ]))
        .unwrap();
        assert_eq!(hash_tree(&a), hash_tree(&b));

        let c = unpack_zip(&zip_of(&[
            ("index.html", b"hi"),
            ("assets/app.js", b"console.log(2)"),
        ]))
        .unwrap();
        assert_ne!(hash_tree(&a), hash_tree(&c));
    }

    #[test]
    fn metadata_reports_size_and_file_count() {
        let tree = unpack_zip(&zip_of(&[
            ("index.html", b"12345"),
            ("assets/app.js", b"123"),
        ]))
        .unwrap();
        let md = BundleMetadata::new(&tree, std::time::UNIX_EPOCH);
        assert_eq!(md.size, 8);
        assert_eq!(md.files, 2);
        assert_eq!(md.hash.len(), 64);
        assert!(md.uploaded_at.starts_with("1970-01-01T00:00:00"));
    }
}
