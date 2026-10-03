//! Per-dataset secret and variable values.
//!
//! Not a schema type. Files live under `schemas/values/`, which
//! `SchemaStore::load` never walks, so `/schema` and `/data` have no route
//! and no type that can return them.
//!
//! ```text
//! {account}/{project}/datasets/{dataset}/secrets/{segment}.yaml
//! {account}/{project}/datasets/{dataset}/variables/{segment}.yaml
//! ```
//!
//! `{segment}` is [`encode_name`] of the declaration reference: bare
//! (`consumer_key`) for this project, qualified (`alice/bricklink.consumer_key`)
//! for a dependency. `%` becomes `%25` and `/` becomes `%2F`; `.` stays. The
//! pair is applied left to right, `%25` first on the way back, so it is
//! injective — a bare name that happens to contain the characters `%2F`
//! cannot collide with a qualified name. A project id is two path segments
//! (`account/project`); the name is always the single encoded segment after
//! `secrets` or `variables`.
//!
//! Writes hold one writer mutex across the adapter write and the cache
//! update, same discipline as [`loco_schema_runtime::InstanceStore`]. The
//! mutex covers both kinds, so a dataset delete cannot interleave with a put
//! of either. Callers that also touch a schema store take that store's lock
//! afterwards, never inside this one. `PINS` is the caller's, taken first.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::RwLock;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use loco_schema_runtime::file_tree::validate_relative_path;
use loco_schema_runtime::Error;
use loco_schema_runtime::SchemaInstance;
use loco_schema_runtime::SchemaPersistence;
use loco_schema_runtime::YamlFsAdapter;

use super::seal::open;
use super::seal::seal;
use super::seal::secret_aad;
use super::seal::OpenError;

const SECRETS: &str = "secrets";
const VARIABLES: &str = "variables";

/// `%` → `%25`, `/` → `%2F`. See the module docs.
pub(crate) fn encode_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            other => out.push(other),
        }
    }
    out
}

/// Inverse of [`encode_name`]. `%25` is consumed before `%2F`.
pub(crate) fn decode_name(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut rest = segment;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("%25") {
            out.push('%');
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("%2F") {
            out.push('/');
            rest = tail;
        } else {
            let ch = rest.chars().next().expect("rest is non-empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

fn check_name(name: &str) -> Result<(), Error> {
    let segment = encode_name(name);
    // A segment `load_all` would skip (`.loco-*`) or reject (`.` / `..`),
    // or a byte that is not a single path segment.
    if name.is_empty()
        || name.chars().any(|c| c.is_control() || c == '\\')
        || segment == "."
        || segment == ".."
        || segment.starts_with('.')
    {
        return Err(Error::InvalidPath(name.to_string()));
    }
    Ok(())
}

fn value_key(project: &str, dataset: &str, kind: &str, name: &str) -> Result<String, Error> {
    check_name(name)?;
    if dataset.is_empty() || dataset.contains('/') || project.split('/').count() != 2 {
        return Err(Error::InvalidPath(format!("{project}/{dataset}/{name}")));
    }
    let key = format!("{project}/datasets/{dataset}/{kind}/{}", encode_name(name));
    validate_relative_path(&key)?;
    Ok(key)
}

fn from_path_kind(path: &str, kind: &str) -> Option<HashMap<String, String>> {
    let segs: Vec<&str> = path.split('/').collect();
    if segs.len() != 6 || segs[2] != "datasets" || segs[4] != kind {
        return None;
    }
    if segs.iter().any(|seg| seg.is_empty()) {
        return None;
    }
    let mut vars = HashMap::new();
    vars.insert("project".to_string(), format!("{}/{}", segs[0], segs[1]));
    vars.insert("dataset".to_string(), segs[3].to_string());
    vars.insert("name".to_string(), decode_name(segs[5]));
    Some(vars)
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SecretRecord {
    /// Logical reference (bare or qualified). The path segment is its
    /// encoding; load trusts the path, not this field.
    name: String,
    nonce: String,
    ciphertext: String,
    updated_at: String,
    #[serde(skip)]
    project: String,
    #[serde(skip)]
    dataset: String,
}

impl SecretRecord {
    pub(crate) fn updated_at(&self) -> &str {
        &self.updated_at
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct VariableRecord {
    name: String,
    value: String,
    updated_at: String,
    #[serde(skip)]
    project: String,
    #[serde(skip)]
    dataset: String,
}

impl VariableRecord {
    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn updated_at(&self) -> &str {
        &self.updated_at
    }
}

#[derive(Default)]
pub(crate) struct NoUpdate;

#[derive(serde::Deserialize)]
struct SecretBody {
    nonce: String,
    ciphertext: String,
    updated_at: String,
}

#[derive(serde::Deserialize)]
struct VariableBody {
    value: String,
    updated_at: String,
}

impl SchemaInstance for SecretRecord {
    type Update = NoUpdate;

    fn to_path(&self) -> String {
        value_key(&self.project, &self.dataset, SECRETS, &self.name)
            .expect("secret name was checked before write")
    }

    fn apply_update(&mut self, _patch: &Self::Update) {}

    fn from_path(path: &str) -> Option<HashMap<String, String>> {
        from_path_kind(path, SECRETS)
    }

    fn from_yaml(yaml: &str, vars: &HashMap<String, String>) -> Result<Self, Error> {
        let body: SecretBody = serde_yaml::from_str(yaml)?;
        Ok(Self {
            name: vars.get("name").cloned().unwrap_or_default(),
            nonce: body.nonce,
            ciphertext: body.ciphertext,
            updated_at: body.updated_at,
            project: vars.get("project").cloned().unwrap_or_default(),
            dataset: vars.get("dataset").cloned().unwrap_or_default(),
        })
    }
}

impl SchemaInstance for VariableRecord {
    type Update = NoUpdate;

    fn to_path(&self) -> String {
        value_key(&self.project, &self.dataset, VARIABLES, &self.name)
            .expect("variable name was checked before write")
    }

    fn apply_update(&mut self, _patch: &Self::Update) {}

    fn from_path(path: &str) -> Option<HashMap<String, String>> {
        from_path_kind(path, VARIABLES)
    }

    fn from_yaml(yaml: &str, vars: &HashMap<String, String>) -> Result<Self, Error> {
        let body: VariableBody = serde_yaml::from_str(yaml)?;
        Ok(Self {
            name: vars.get("name").cloned().unwrap_or_default(),
            value: body.value,
            updated_at: body.updated_at,
            project: vars.get("project").cloned().unwrap_or_default(),
            dataset: vars.get("dataset").cloned().unwrap_or_default(),
        })
    }
}

/// `Err(NotDurable)` means the new bytes are already in place.
fn persisted(result: Result<(), Error>) -> Result<Result<(), Error>, Error> {
    match result {
        Err(e @ Error::NotDurable(_)) => Ok(Err(e)),
        Err(e) => Err(e),
        Ok(()) => Ok(Ok(())),
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub struct ConfigValueStore {
    dir: PathBuf,
    secrets: RwLock<BTreeMap<String, Arc<SecretRecord>>>,
    variables: RwLock<BTreeMap<String, Arc<VariableRecord>>>,
    secret_adapter: Arc<YamlFsAdapter<SecretRecord>>,
    variable_adapter: Arc<YamlFsAdapter<VariableRecord>>,
    writer: Mutex<()>,
}

impl ConfigValueStore {
    /// Load every value file. A YAML body that does not parse fails the load,
    /// same as a schema instance: a local server should not boot on a lie.
    /// A missing directory is an empty store.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let secret_adapter = Arc::new(YamlFsAdapter::new(dir.to_path_buf()));
        let variable_adapter = Arc::new(YamlFsAdapter::new(dir.to_path_buf()));
        let mut secrets = BTreeMap::new();
        for (key, record) in secret_adapter.load_all()? {
            secrets.insert(key, Arc::new(record));
        }
        let mut variables = BTreeMap::new();
        for (key, record) in variable_adapter.load_all()? {
            variables.insert(key, Arc::new(record));
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            secrets: RwLock::new(secrets),
            variables: RwLock::new(variables),
            secret_adapter,
            variable_adapter,
            writer: Mutex::new(()),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn lock_writer(&self) -> MutexGuard<'_, ()> {
        self.writer.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub(crate) fn secret(
        &self,
        project: &str,
        dataset: &str,
        name: &str,
    ) -> Option<Arc<SecretRecord>> {
        let key = value_key(project, dataset, SECRETS, name).ok()?;
        self.secrets.read().unwrap().get(&key).cloned()
    }

    pub(crate) fn variable(
        &self,
        project: &str,
        dataset: &str,
        name: &str,
    ) -> Option<Arc<VariableRecord>> {
        let key = value_key(project, dataset, VARIABLES, name).ok()?;
        self.variables.read().unwrap().get(&key).cloned()
    }

    /// Encrypt `plaintext` and replace any value already stored for `name`.
    /// The plaintext is not a field of the record, so the adapter cannot
    /// write it.
    pub(crate) fn put_secret(
        &self,
        project: &str,
        dataset: &str,
        name: &str,
        plaintext: &str,
        key: &[u8; 32],
    ) -> Result<Arc<SecretRecord>, Error> {
        let path_key = value_key(project, dataset, SECRETS, name)?;
        let aad = secret_aad(project, dataset, name);
        let sealed = seal(key, &aad, plaintext.as_bytes())
            .map_err(|_| Error::Io(std::io::Error::other("aes-256-gcm encrypt failed")))?;
        let record = SecretRecord {
            name: name.to_string(),
            nonce: STANDARD.encode(sealed.nonce),
            ciphertext: STANDARD.encode(sealed.ciphertext),
            updated_at: now(),
            project: project.to_string(),
            dataset: dataset.to_string(),
        };
        let _writer = self.lock_writer();
        let written = persisted(self.secret_adapter.write(&path_key, &record))?;
        let arc = Arc::new(record);
        self.secrets.write().unwrap().insert(path_key, arc.clone());
        written.map(|()| arc)
    }

    pub(crate) fn put_variable(
        &self,
        project: &str,
        dataset: &str,
        name: &str,
        value: &str,
    ) -> Result<Arc<VariableRecord>, Error> {
        let path_key = value_key(project, dataset, VARIABLES, name)?;
        let record = VariableRecord {
            name: name.to_string(),
            value: value.to_string(),
            updated_at: now(),
            project: project.to_string(),
            dataset: dataset.to_string(),
        };
        let _writer = self.lock_writer();
        let written = persisted(self.variable_adapter.write(&path_key, &record))?;
        let arc = Arc::new(record);
        self.variables
            .write()
            .unwrap()
            .insert(path_key, arc.clone());
        written.map(|()| arc)
    }

    /// Open a stored secret. No HTTP route calls this; a handler that needs
    /// the plaintext (#102) will. A wrong key is [`OpenError::Failed`], not
    /// the plaintext.
    pub fn decrypt_secret(
        &self,
        project: &str,
        dataset: &str,
        name: &str,
        key: &[u8; 32],
    ) -> Result<String, OpenError> {
        let record = self
            .secret(project, dataset, name)
            .ok_or(OpenError::NotFound)?;
        let nonce = STANDARD
            .decode(record.nonce.as_bytes())
            .map_err(|_| OpenError::Failed)?;
        let ciphertext = STANDARD
            .decode(record.ciphertext.as_bytes())
            .map_err(|_| OpenError::Failed)?;
        let aad = secret_aad(project, dataset, name);
        let plain = open(key, &aad, &nonce, &ciphertext)?;
        String::from_utf8(plain).map_err(|_| OpenError::Failed)
    }

    pub fn delete_secret(&self, project: &str, dataset: &str, name: &str) -> Result<(), Error> {
        let key = value_key(project, dataset, SECRETS, name)?;
        self.delete_key(&key, true)
    }

    pub fn delete_variable(&self, project: &str, dataset: &str, name: &str) -> Result<(), Error> {
        let key = value_key(project, dataset, VARIABLES, name)?;
        self.delete_key(&key, false)
    }

    fn delete_key(&self, key: &str, secret: bool) -> Result<(), Error> {
        let _writer = self.lock_writer();
        let present = if secret {
            self.secrets.read().unwrap().contains_key(key)
        } else {
            self.variables.read().unwrap().contains_key(key)
        };
        if !present {
            return Err(Error::NotFound(key.to_string()));
        }
        if secret {
            self.secret_adapter.delete(key)?;
            self.secrets.write().unwrap().remove(key);
        } else {
            self.variable_adapter.delete(key)?;
            self.variables.write().unwrap().remove(key);
        }
        Ok(())
    }

    /// Both kinds under one dataset. An empty dataset is success.
    pub fn delete_dataset(&self, project: &str, dataset: &str) -> Result<(), Error> {
        let prefix = format!("{project}/datasets/{dataset}/");
        self.delete_by_prefix(&prefix).map(|_| ())
    }

    /// Delete every value whose key starts with `prefix`. One failure leaves
    /// that key cached (it is still on disk) and is returned after the rest
    /// are attempted.
    pub fn delete_by_prefix(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let _writer = self.lock_writer();
        let mut keys = self.keys_locked(prefix);
        keys.sort();
        let mut deleted = Vec::new();
        let mut first_err = None;
        for key in keys {
            let secret = self.secrets.read().unwrap().contains_key(&key);
            let result = if secret {
                self.secret_adapter.delete(&key)
            } else {
                self.variable_adapter.delete(&key)
            };
            match result {
                Ok(()) => {
                    if secret {
                        self.secrets.write().unwrap().remove(&key);
                    } else {
                        self.variables.write().unwrap().remove(&key);
                    }
                    deleted.push(key);
                }
                Err(err) => {
                    first_err.get_or_insert(err);
                }
            }
        }
        match first_err {
            Some(err) => Err(err),
            None => Ok(deleted),
        }
    }

    pub fn list_prefix(&self, prefix: &str) -> Vec<String> {
        let mut keys = self.keys_locked(prefix);
        keys.sort();
        keys
    }

    fn keys_locked(&self, prefix: &str) -> Vec<String> {
        let secrets = self.secrets.read().unwrap();
        let variables = self.variables.read().unwrap();
        secrets
            .range(prefix.to_string()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .map(|(key, _)| key.clone())
            .chain(
                variables
                    .range(prefix.to_string()..)
                    .take_while(|(key, _)| key.starts_with(prefix))
                    .map(|(key, _)| key.clone()),
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &str = "plain secret: do-not-store";
    const KEY: [u8; 32] = [9u8; 32];

    fn store() -> (tempfile::TempDir, ConfigValueStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigValueStore::load(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn name_encoding_roundtrips_and_does_not_collide() {
        assert_eq!(encode_name("consumer_key"), "consumer_key");
        assert_eq!(
            encode_name("alice/bricklink.consumer_key"),
            "alice%2Fbricklink.consumer_key"
        );
        assert_eq!(
            encode_name("alice%2Fbricklink.consumer_key"),
            "alice%252Fbricklink.consumer_key"
        );
        for name in [
            "consumer_key",
            "alice/bricklink.consumer_key",
            "alice%2Fbricklink.consumer_key",
            "a%b/c",
            "dots.stay.put",
        ] {
            assert_eq!(decode_name(&encode_name(name)), name, "{name}");
        }
        assert_ne!(
            encode_name("alice/bricklink.consumer_key"),
            encode_name("alice%2Fbricklink.consumer_key")
        );
    }

    #[test]
    fn stored_secret_hides_plaintext_and_wrong_key_fails() {
        let (dir, store) = store();
        store
            .put_secret(
                "alice/shop",
                "dev",
                "alice/bricklink.consumer_key",
                PLAIN,
                &KEY,
            )
            .unwrap();
        let path = dir
            .path()
            .join("alice/shop/datasets/dev/secrets/alice%2Fbricklink.consumer_key.yaml");
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes
                .windows(PLAIN.len())
                .any(|window| window == PLAIN.as_bytes()),
            "plaintext landed on disk: {}",
            String::from_utf8_lossy(&bytes)
        );
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("ciphertext"));
        assert!(!text.contains("value:"));

        let wrong = [8u8; 32];
        let err = store
            .decrypt_secret("alice/shop", "dev", "alice/bricklink.consumer_key", &wrong)
            .unwrap_err();
        assert!(matches!(err, OpenError::Failed));
        assert!(!err.to_string().contains(PLAIN));

        let opened = store
            .decrypt_secret("alice/shop", "dev", "alice/bricklink.consumer_key", &KEY)
            .unwrap();
        assert_eq!(opened, PLAIN);
    }

    #[test]
    fn overwrite_uses_a_new_nonce_and_reloads() {
        let (dir, store) = store();
        let first = store
            .put_secret("alice/shop", "dev", "token", PLAIN, &KEY)
            .unwrap();
        let second = store
            .put_secret(
                "alice/shop",
                "dev",
                "token",
                "other secret: also hidden",
                &KEY,
            )
            .unwrap();
        assert_ne!(first.nonce, second.nonce);
        drop(store);

        let reloaded = ConfigValueStore::load(dir.path()).unwrap();
        assert_eq!(
            reloaded
                .decrypt_secret("alice/shop", "dev", "token", &KEY)
                .unwrap(),
            "other secret: also hidden"
        );
        let bytes = std::fs::read(
            dir.path()
                .join("alice/shop/datasets/dev/secrets/token.yaml"),
        )
        .unwrap();
        assert!(!bytes.windows(PLAIN.len()).any(|w| w == PLAIN.as_bytes()));
    }

    #[test]
    fn variable_roundtrips_ambiguous_scalars_and_stays_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        for value in ["true", "null", "1", "", "yes", "Lot: 12"] {
            let store = ConfigValueStore::load(dir.path()).unwrap();
            store
                .put_variable("alice/shop", "dev", "label_prefix", value)
                .unwrap();
            drop(store);
            // Reload, so a YAML scalar that parsed as bool, null, or number
            // fails here instead of only in the cache.
            let reloaded = ConfigValueStore::load(dir.path()).unwrap();
            assert_eq!(
                reloaded
                    .variable("alice/shop", "dev", "label_prefix")
                    .unwrap()
                    .value(),
                value,
                "{value:?}"
            );
        }
        let text = std::fs::read_to_string(
            dir.path()
                .join("alice/shop/datasets/dev/variables/label_prefix.yaml"),
        )
        .unwrap();
        assert!(
            text.contains("Lot: 12"),
            "variable value should stay readable plaintext: {text}"
        );
    }

    #[test]
    fn dataset_delete_removes_both_kinds_and_leaves_the_other_dataset() {
        let (_dir, store) = store();
        store
            .put_secret("alice/shop", "dev", "token", PLAIN, &KEY)
            .unwrap();
        store
            .put_variable("alice/shop", "dev", "region", "us")
            .unwrap();
        store
            .put_variable("alice/shop", "other", "region", "eu")
            .unwrap();
        store.delete_dataset("alice/shop", "dev").unwrap();
        assert!(store.secret("alice/shop", "dev", "token").is_none());
        assert!(store.variable("alice/shop", "dev", "region").is_none());
        assert_eq!(
            store
                .variable("alice/shop", "other", "region")
                .unwrap()
                .value(),
            "eu"
        );
        // A second delete of an empty dataset is success, not 404.
        store.delete_dataset("alice/shop", "dev").unwrap();
    }

    #[test]
    fn rejects_a_name_load_would_skip() {
        let (_dir, store) = store();
        let err = store
            .put_variable("alice/shop", "dev", ".loco-hidden", "x")
            .unwrap_err();
        assert!(matches!(err, Error::InvalidPath(_)));
    }
}
