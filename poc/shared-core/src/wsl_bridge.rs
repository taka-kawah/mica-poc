//! WSL bridge stub: the stdio JSON-RPC protocol between the Windows
//! GUI and the WSL backend.
//!
//! The real design (Notion §11, Zed-style) is: the Windows GUI starts
//! a small server process inside WSL with `wsl.exe`, and talks to it
//! over the child process's standard input/output. Messages are
//! length-framed (4-byte little-endian length, then one JSON object)
//! and use JSON-RPC. SSH is explicitly NOT used for WSL.
//!
//! This module implements the *protocol* — framing, request/response
//! shapes, and the method names — plus an in-process local-filesystem
//! backend, so the PoCs can run and be tested on Linux, where there
//! is no `wsl.exe`. The Windows transport (spawning `wsl.exe -d
//! <distro> --exec ~/.mica_server/mica-server-<version>`) is
//! documented in the crate README and is NOT faked as working here.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Methods the GUI can ask the backend to perform.
pub const METHOD_OPEN_FOLDER: &str = "open_folder";
pub const METHOD_LIST_DIR: &str = "list_dir";
pub const METHOD_READ_FILE: &str = "read_file";
pub const METHOD_WRITE_FILE: &str = "write_file";
pub const METHOD_RENAME: &str = "rename";
pub const METHOD_DELETE: &str = "delete";
pub const METHOD_SEARCH: &str = "search";

/// Notifications the backend pushes to the GUI.
pub const NOTIFICATION_FS_CHANGED: &str = "fs_changed";

/// One JSON-RPC request from the GUI to the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRequest {
    /// JSON-RPC version marker, always "2.0".
    pub jsonrpc: String,
    /// Request id (echoed in the response).
    pub id: u64,
    /// Method name, one of the `METHOD_*` constants.
    pub method: String,
    /// Method arguments as a JSON value.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// One JSON-RPC response from the backend to the GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResponse {
    /// JSON-RPC version marker, always "2.0".
    pub jsonrpc: String,
    /// Id of the request this answers.
    pub id: u64,
    /// Result on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error on failure: `{"code": <number>, "message": "<text>"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// Encode one message as a length-framed frame:
/// 4-byte little-endian length, then the JSON bytes.
pub fn encode_frame(message: &serde_json::Value) -> Vec<u8> {
    let json = serde_json::to_vec(message).expect("message is serialisable");
    let mut frame = Vec::with_capacity(4 + json.len());
    frame.extend_from_slice(&(json.len() as u32).to_le_bytes());
    frame.extend_from_slice(&json);
    frame
}

/// Decode one length-framed message from a reader.
/// Maximum frame size accepted by [`decode_frame`].
///
/// The length prefix comes from the other end of the stdio bridge, so
/// without a cap a corrupt length of up to 4 GiB would be allocated
/// before the read fails. 64 MiB is far above any legitimate frame
/// (bridge messages are small JSON requests and replies) and far below
/// the 4 GiB worst case.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Returns `Ok(None)` on a clean end of stream.
pub fn decode_frame(reader: &mut impl Read) -> std::io::Result<Option<serde_json::Value>> {
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame length {length} exceeds the maximum of {MAX_FRAME_BYTES} bytes"),
        ));
    }
    let mut json_bytes = vec![0u8; length];
    reader.read_exact(&mut json_bytes)?;
    let value: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok(Some(value))
}

/// Write one framed message to a writer.
pub fn write_frame(writer: &mut impl Write, message: &serde_json::Value) -> std::io::Result<()> {
    writer.write_all(&encode_frame(message))?;
    writer.flush()
}

/// The backend side: answers bridge requests against a local folder.
/// On Linux the PoCs use this directly (in-process). On Windows the
/// same requests would travel over stdio to the WSL server process.
pub struct LocalBridgeBackend {
    root: PathBuf,
}

impl LocalBridgeBackend {
    /// Create a backend rooted at `root`. Paths in requests are
    /// resolved below this root; `..` escapes are rejected.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Answer one request, returning a response with the same id.
    pub fn handle(&self, request: &BridgeRequest) -> BridgeResponse {
        let result = match request.method.as_str() {
            METHOD_OPEN_FOLDER => self.open_folder(&request.params),
            METHOD_LIST_DIR => self.list_dir(&request.params),
            METHOD_READ_FILE => self.read_file(&request.params),
            METHOD_WRITE_FILE => self.write_file(&request.params),
            METHOD_RENAME => self.rename(&request.params),
            METHOD_DELETE => self.delete(&request.params),
            METHOD_SEARCH => self.search(&request.params),
            _ => Err(format!("unknown method: {}", request.method)),
        };
        match result {
            Ok(value) => BridgeResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(value),
                error: None,
            },
            Err(message) => BridgeResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(serde_json::json!({"code": -32000, "message": message})),
            },
        }
    }

    /// Resolve a request path below the root, rejecting escapes.
    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let candidate = self.root.join(raw);
        let normalised = normalise_path(&candidate);
        if !normalised.starts_with(normalise_path(&self.root)) {
            return Err(format!("path escapes the opened folder: {raw}"));
        }
        Ok(normalised)
    }

    fn open_folder(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = params["path"]
            .as_str()
            .ok_or("open_folder needs a string 'path'")?;
        let resolved = self.resolve(path)?;
        if !resolved.is_dir() {
            return Err(format!("not a folder: {path}"));
        }
        Ok(serde_json::json!({"path": resolved.to_string_lossy()}))
    }

    fn list_dir(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = params["path"]
            .as_str()
            .ok_or("list_dir needs a string 'path'")?;
        let resolved = self.resolve(path)?;
        let mut names: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&resolved).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        names.sort();
        Ok(serde_json::json!({"entries": names}))
    }

    fn read_file(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = params["path"]
            .as_str()
            .ok_or("read_file needs a string 'path'")?;
        let resolved = self.resolve(path)?;
        let content = std::fs::read_to_string(&resolved).map_err(|error| error.to_string())?;
        Ok(serde_json::json!({"content": content}))
    }

    fn write_file(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = params["path"]
            .as_str()
            .ok_or("write_file needs a string 'path'")?;
        let content = params["content"]
            .as_str()
            .ok_or("write_file needs a string 'content'")?;
        let resolved = self.resolve(path)?;
        crate::auto_save::save_atomically(&resolved, content).map_err(|error| error.to_string())?;
        Ok(serde_json::json!({"ok": true}))
    }

    /// `rename {from, to}`: move a file or folder. Both paths must stay
    /// inside the opened folder, and an existing target is never
    /// overwritten (the GUI must delete it first, explicitly).
    fn rename(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let from = params["from"]
            .as_str()
            .ok_or("rename needs a string 'from'")?;
        let to = params["to"].as_str().ok_or("rename needs a string 'to'")?;
        let resolved_from = self.resolve(from)?;
        let resolved_to = self.resolve(to)?;
        if resolved_from == normalise_path(&self.root) {
            return Err("cannot rename the opened folder itself".to_string());
        }
        if std::fs::symlink_metadata(&resolved_from).is_err() {
            return Err(format!("no such file or folder: {from}"));
        }
        if std::fs::symlink_metadata(&resolved_to).is_ok() {
            return Err(format!("target already exists: {to}"));
        }
        std::fs::rename(&resolved_from, &resolved_to).map_err(|error| error.to_string())?;
        Ok(serde_json::json!({"ok": true}))
    }

    /// `delete {path}`: remove a file, or a folder with its contents.
    /// The opened folder itself cannot be deleted. Symlinks are
    /// removed as links (their target is never followed).
    fn delete(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = params["path"]
            .as_str()
            .ok_or("delete needs a string 'path'")?;
        let resolved = self.resolve(path)?;
        if resolved == normalise_path(&self.root) {
            return Err("cannot delete the opened folder itself".to_string());
        }
        let metadata = std::fs::symlink_metadata(&resolved).map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            std::fs::remove_dir_all(&resolved)
        } else {
            std::fs::remove_file(&resolved)
        }
        .map_err(|error| error.to_string())?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn search(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let query = params["query"]
            .as_str()
            .ok_or("search needs a string 'query'")?;
        let mut matches: Vec<String> = Vec::new();
        search_recursive(&self.root, query, &mut matches);
        matches.sort();
        Ok(serde_json::json!({"matches": matches}))
    }
}

fn search_recursive(dir: &Path, query: &str, matches: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) != Some(".git") {
                search_recursive(&path, query, matches);
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if content.contains(query) {
                matches.push(path.to_string_lossy().to_string());
            }
        }
    }
}

/// Remove `.` and resolve `..` lexically (no filesystem access, so
/// symlinks are not followed — good enough for the PoC's guard).
fn normalise_path(path: &Path) -> PathBuf {
    let mut normalised = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalised.pop();
            }
            std::path::Component::CurDir => {}
            other => normalised.push(other.as_os_str()),
        }
    }
    normalised
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trip() {
        let message = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "list_dir"});
        let frame = encode_frame(&message);
        let mut cursor = Cursor::new(frame);
        let decoded = decode_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(decoded, message);
        assert!(decode_frame(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn oversized_frame_length_is_rejected_before_allocating() {
        // Only the 4-byte length prefix, no payload. If decode_frame
        // allocated and tried to read the body, it would fail with
        // UnexpectedEof; InvalidData proves the cap check ran first.
        let declared = (MAX_FRAME_BYTES + 1) as u32;
        let mut cursor = Cursor::new(declared.to_le_bytes().to_vec());
        let error = decode_frame(&mut cursor).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("exceeds the maximum"),
            "unexpected message: {error}"
        );
        // Nothing past the length prefix was consumed.
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn small_frame_still_decodes() {
        let message = serde_json::json!({"jsonrpc": "2.0", "id": 7, "result": {"ok": true}});
        let mut cursor = Cursor::new(encode_frame(&message));
        assert_eq!(decode_frame(&mut cursor).unwrap().unwrap(), message);
    }

    #[test]
    fn backend_read_write_and_escape_guard() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "# Hello").unwrap();
        let backend = LocalBridgeBackend::new(dir.path());
        let request = BridgeRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: METHOD_READ_FILE.to_string(),
            params: serde_json::json!({"path": "note.md"}),
        };
        let response = backend.handle(&request);
        assert_eq!(response.result.unwrap()["content"], "# Hello");
        let escape = BridgeRequest {
            jsonrpc: "2.0".to_string(),
            id: 2,
            method: METHOD_READ_FILE.to_string(),
            params: serde_json::json!({"path": "../secret.md"}),
        };
        assert!(backend.handle(&escape).error.is_some());
    }

    fn request(id: u64, method: &str, params: serde_json::Value) -> BridgeRequest {
        BridgeRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn backend_rename_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.md"), "# Old").unwrap();
        std::fs::write(dir.path().join("taken.md"), "# Taken").unwrap();
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        std::fs::write(dir.path().join("folder/inner.md"), "x").unwrap();
        let backend = LocalBridgeBackend::new(dir.path());

        // Rename moves the file; the response echoes the id.
        let response = backend.handle(&request(
            1,
            METHOD_RENAME,
            serde_json::json!({"from": "old.md", "to": "new.md"}),
        ));
        assert_eq!(response.id, 1);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert!(!dir.path().join("old.md").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.md")).unwrap(),
            "# Old"
        );
        // Never overwrite an existing target.
        let clash = backend.handle(&request(
            2,
            METHOD_RENAME,
            serde_json::json!({"from": "new.md", "to": "taken.md"}),
        ));
        assert!(clash.error.is_some());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("taken.md")).unwrap(),
            "# Taken"
        );
        // Escapes are rejected for both ends.
        let escape = backend.handle(&request(
            3,
            METHOD_RENAME,
            serde_json::json!({"from": "new.md", "to": "../outside.md"}),
        ));
        assert!(escape.error.is_some());
        assert!(dir.path().join("new.md").exists());

        // Delete removes a file and a folder with its contents.
        let deleted = backend.handle(&request(
            4,
            METHOD_DELETE,
            serde_json::json!({"path": "new.md"}),
        ));
        assert!(deleted.error.is_none(), "{:?}", deleted.error);
        assert!(!dir.path().join("new.md").exists());
        let deleted_folder = backend.handle(&request(
            5,
            METHOD_DELETE,
            serde_json::json!({"path": "folder"}),
        ));
        assert!(deleted_folder.error.is_none(), "{:?}", deleted_folder.error);
        assert!(!dir.path().join("folder").exists());
        // The opened folder itself and escapes cannot be deleted.
        for path in [".", "", "../x"] {
            let refused = backend.handle(&request(
                6,
                METHOD_DELETE,
                serde_json::json!({"path": path}),
            ));
            assert!(refused.error.is_some(), "delete {path:?} must fail");
        }
        assert!(dir.path().exists());
        // Missing file: an error, not a panic.
        let missing = backend.handle(&request(
            7,
            METHOD_DELETE,
            serde_json::json!({"path": "nope.md"}),
        ));
        assert!(missing.error.is_some());
    }
}
