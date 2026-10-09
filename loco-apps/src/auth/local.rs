use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, RwLock};

use chrono::{DateTime, Duration, Utc};
use loco_schema_runtime::{is_temp_name, write_file_atomic, Error as StoreError};
use serde::{Deserialize, Serialize};

use super::secret;
use super::{
    Account, AccountType, ApiKey, ApiKeyInfo, AuthAdapter, AuthError, AuthSession, AuthUser,
    CreateUserRequest, LoginCredentials, OrgMember, OrgRole, ProjectMember, ProjectRole,
    UpdateUserRequest, PUBLIC_USERNAME, TEST_PASSWORD,
};
use crate::http::names::{check_slug, member_handle_charset_sentence, slug_charset_ok};

#[derive(Serialize, Deserialize, Clone)]
struct StoredAccount {
    handle: String,
    #[serde(rename = "type")]
    account_type: AccountType,
    created_at: String,
}

/// 1:1 with a person account. Org accounts have no identity and cannot log in.
#[derive(Serialize, Deserialize, Clone)]
struct StoredIdentity {
    id: String,
    handle: String,
    name: String,
    /// Argon2id PHC string, never the password itself. `alias` reads files
    /// written before hashing landed; [`LocalAuthAdapter::load_from_disk`]
    /// re-hashes those in place.
    #[serde(rename = "password_hash", alias = "password")]
    password_hash: String,
    created_at: String,
    last_login_at: Option<String>,
}

/// How long a login is good for. Sessions are absolute — there is no refresh,
/// so past this point the client logs in again.
pub const SESSION_TTL_DAYS: i64 = 7;

#[derive(Serialize, Deserialize, Clone)]
struct StoredSession {
    token: String,
    identity_id: String,
    created_at: String,
    /// Absolute expiry, RFC 3339. Sessions written before expiry landed have
    /// no such field; those fall back to `created_at` + [`SESSION_TTL_DAYS`].
    #[serde(default)]
    expires_at: Option<String>,
}

impl StoredSession {
    fn new(token: String, identity_id: String, now: DateTime<Utc>) -> Self {
        StoredSession {
            token,
            identity_id,
            created_at: now.to_rfc3339(),
            expires_at: Some((now + Duration::days(SESSION_TTL_DAYS)).to_rfc3339()),
        }
    }

    /// A session whose dates cannot be parsed is treated as expired: we cannot
    /// tell how old it is, so it does not get to authenticate anything.
    fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self.expiry() {
            Some(expiry) => now >= expiry,
            None => true,
        }
    }

    fn expiry(&self) -> Option<DateTime<Utc>> {
        match &self.expires_at {
            Some(expires_at) => parse_rfc3339(expires_at),
            None => parse_rfc3339(&self.created_at).map(|at| at + Duration::days(SESSION_TTL_DAYS)),
        }
    }
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredApiKey {
    id: String,
    /// SHA-256 of the key, never the key itself. The plaintext key is returned
    /// once, from [`LocalAuthAdapter::create_api_key`].
    key_hash: String,
    identity_id: String,
    label: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredOrgMember {
    org: String,
    handle: String,
    role: OrgRole,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredProjectMember {
    project: String,
    handle: String,
    role: ProjectRole,
    created_at: String,
}

/// Filesystem layout (global, not site-scoped):
///
/// ```text
/// {base}/accounts/{handle}.json
/// {base}/identities/{handle}.json
/// {base}/sessions/{token}.json
/// {base}/api_keys/{id}.json
/// {base}/org_members/{org}/{handle}.json
/// {base}/project_members/{account}/{project}/{handle}.json
/// ```
///
/// Each file is replaced through [`write_file_atomic`] (temp sibling, fsync,
/// rename, fsync the directory). A crash leaves the previous file, never a
/// truncated one. Temp siblings are named `.loco-*`; load skips that prefix.
///
/// The map is updated only once the new bytes are at the target path. A
/// directory fsync failure ([`StoreError::NotDurable`]) still updates the map,
/// because the rename has already happened and cache and disk must agree, and
/// the adapter method returns that as [`AuthError::Internal`].
///
/// Each map has a writer mutex held across re-read, persist, and the cache
/// update, and the `RwLock` write is only that update, so a reader never waits
/// on an fsync. The same split as [`loco_schema_runtime::InstanceStore`]. Two
/// maps are taken `accounts` then `identities`, and never the other way.
pub struct LocalAuthAdapter {
    base_dir: PathBuf,
    /// Login of an unknown handle creates a person account. Off unless the
    /// caller asks for it — see [`LocalAuthAdapter::new`].
    auto_create: bool,
    accounts: RwLock<HashMap<String, StoredAccount>>,
    accounts_writer: Mutex<()>,
    identities: RwLock<HashMap<String, StoredIdentity>>, // handle → identity
    identities_writer: Mutex<()>,
    sessions: RwLock<HashMap<String, StoredSession>>, // token → session
    sessions_writer: Mutex<()>,
    api_keys: RwLock<HashMap<String, StoredApiKey>>, // id → key
    api_keys_writer: Mutex<()>,
    org_members: RwLock<HashMap<(String, String), StoredOrgMember>>, // (org, handle)
    org_members_writer: Mutex<()>,
    project_members: RwLock<HashMap<(String, String), StoredProjectMember>>, // (project, handle)
    project_members_writer: Mutex<()>,
    /// Test seams. The first barrier fires at the pause; the second lets the
    /// writer continue. Production builds have neither field.
    ///
    /// `pause_before_write` is login, after the password check, and
    /// `update_project_member`, between its read and its persist.
    /// `pause_after_account_delete` is `delete_user`, between the account
    /// delete and the identity delete. The login test calls `delete_user`
    /// while `pause_before_write` is armed, so the two seams stay separate.
    #[cfg(test)]
    pause_before_write: Mutex<Option<std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)>>>,
    #[cfg(test)]
    pause_after_account_delete:
        Mutex<Option<std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)>>>,
}

/// A write that left the new bytes at the target path.
///
/// [`Wrote::NotDurable`] is the rename-succeeded, directory-fsync-failed case
/// from [`write_file_atomic`]. Callers insert into the cache and then surface
/// the error. A hard failure is [`Err`] and must not touch the cache.
#[must_use]
enum Wrote {
    Durable,
    NotDurable(String),
}

fn wrote(result: Result<(), StoreError>) -> Result<Wrote, AuthError> {
    match result {
        Ok(()) => Ok(Wrote::Durable),
        Err(StoreError::NotDurable(err)) => {
            Ok(Wrote::NotDurable(StoreError::NotDurable(err).to_string()))
        }
        Err(err) => Err(AuthError::Internal(err.to_string())),
    }
}

fn finish(wrote: Wrote) -> Result<(), AuthError> {
    match wrote {
        Wrote::Durable => Ok(()),
        Wrote::NotDurable(message) => Err(AuthError::Internal(message)),
    }
}

/// Insert only when `write` put the bytes at the target path, then return
/// [`Wrote::NotDurable`] as [`AuthError::Internal`].
fn commit(write: Result<Wrote, AuthError>, insert: impl FnOnce()) -> Result<(), AuthError> {
    match write {
        Ok(outcome) => {
            insert();
            finish(outcome)
        }
        Err(err) => Err(err),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<Wrote, AuthError> {
    let parent = path.parent().ok_or_else(|| {
        AuthError::Internal(format!("auth path {} has no parent", path.display()))
    })?;
    std::fs::create_dir_all(parent).map_err(|err| AuthError::Internal(err.to_string()))?;
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|err| AuthError::Internal(err.to_string()))?;
    wrote(write_file_atomic(path, &bytes))
}

/// A missing file is already gone. Anything else, including a directory the
/// process cannot write, is an error so the cache is left alone.
fn remove_json(path: &Path) -> Result<(), AuthError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(AuthError::Internal(err.to_string())),
    }
}

fn is_temp_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_temp_name)
}

/// Serializes mutations of one map. It guards no data of its own, so a panic
/// while it was held leaves nothing inconsistent and the poison is ignored.
fn lock_writer(writer: &Mutex<()>) -> MutexGuard<'_, ()> {
    writer.lock().unwrap_or_else(|err| err.into_inner())
}

impl LocalAuthAdapter {
    /// `auto_create` off is the production default. An unknown handle would
    /// otherwise take the `{handle}/*` namespace, because owning the person
    /// account implies developer on it. The server passes the flag from
    /// [`crate::auth::AuthConfig`].
    pub fn new(base_dir: &Path, auto_create: bool) -> Self {
        let adapter = LocalAuthAdapter {
            base_dir: base_dir.to_path_buf(),
            auto_create,
            accounts: RwLock::new(HashMap::new()),
            accounts_writer: Mutex::new(()),
            identities: RwLock::new(HashMap::new()),
            identities_writer: Mutex::new(()),
            sessions: RwLock::new(HashMap::new()),
            sessions_writer: Mutex::new(()),
            api_keys: RwLock::new(HashMap::new()),
            api_keys_writer: Mutex::new(()),
            org_members: RwLock::new(HashMap::new()),
            org_members_writer: Mutex::new(()),
            project_members: RwLock::new(HashMap::new()),
            project_members_writer: Mutex::new(()),
            #[cfg(test)]
            pause_before_write: Mutex::new(None),
            #[cfg(test)]
            pause_after_account_delete: Mutex::new(None),
        };
        adapter.load_from_disk().expect("failed to load auth store");
        adapter
            .seed_defaults()
            .expect("failed to seed default accounts");
        adapter
    }

    fn load_from_disk(&self) -> Result<(), AuthError> {
        self.load_json_dir("accounts", |this, account: StoredAccount| {
            this.accounts
                .write()
                .unwrap()
                .insert(account.handle.clone(), account);
        });
        self.load_identities()?;
        self.load_json_dir("sessions", |this, session: StoredSession| {
            this.sessions
                .write()
                .unwrap()
                .insert(session.token.clone(), session);
        });
        self.sweep_expired_sessions(Utc::now())?;
        self.load_api_keys()?;
        self.load_member_tree("org_members", |this, member: StoredOrgMember| {
            this.org_members
                .write()
                .unwrap()
                .insert((member.org.clone(), member.handle.clone()), member);
        });
        self.load_member_tree("project_members", |this, member: StoredProjectMember| {
            this.project_members
                .write()
                .unwrap()
                .insert((member.project.clone(), member.handle.clone()), member);
        });
        Ok(())
    }

    /// Plaintext passwords written before hashing are rewritten in place.
    /// The hashed identity enters the cache only after that write lands.
    fn load_identities(&self) -> Result<(), AuthError> {
        for identity in self.read_json_dir::<StoredIdentity>("identities") {
            let mut identity = identity;
            if !secret::is_password_hash(&identity.password_hash) {
                identity.password_hash = secret::hash_password(&identity.password_hash);
                let wrote = self.persist_identity(&identity)?;
                self.identities
                    .write()
                    .unwrap()
                    .insert(identity.handle.clone(), identity);
                finish(wrote)?;
            } else {
                self.identities
                    .write()
                    .unwrap()
                    .insert(identity.handle.clone(), identity);
            }
        }
        Ok(())
    }

    /// Same upgrade as identities: the field used to hold the key itself.
    fn load_api_keys(&self) -> Result<(), AuthError> {
        for key in self.read_json_dir::<StoredApiKey>("api_keys") {
            let mut key = key;
            if !secret::is_api_key_hash(&key.key_hash) {
                key.key_hash = secret::hash_api_key(&key.key_hash);
                let wrote = self.persist_api_key(&key)?;
                self.api_keys.write().unwrap().insert(key.id.clone(), key);
                finish(wrote)?;
            } else {
                self.api_keys.write().unwrap().insert(key.id.clone(), key);
            }
        }
        Ok(())
    }

    /// JSON files in one directory, skipping temp siblings and files that do
    /// not parse. A missing directory is an empty store.
    fn read_json_dir<T: for<'de> Deserialize<'de>>(&self, dirname: &str) -> Vec<T> {
        let dir = self.base_dir.join(dirname);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if is_temp_entry(&path) || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(value) = serde_json::from_str::<T>(&contents) {
                out.push(value);
            }
        }
        out
    }

    fn load_member_tree<T: for<'de> Deserialize<'de>>(
        &self,
        dirname: &str,
        insert: impl Fn(&Self, T),
    ) {
        let dir = self.base_dir.join(dirname);
        Self::walk_json_files(&dir, |path| {
            let Ok(contents) = std::fs::read_to_string(path) else {
                return;
            };
            if let Ok(value) = serde_json::from_str::<T>(&contents) {
                insert(self, value);
            }
        });
    }

    fn walk_json_files(dir: &Path, visit: impl FnMut(&Path)) {
        let mut visit = visit;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if is_temp_entry(&path) {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    visit(&path);
                }
            }
        }
    }

    fn load_json_dir<T: for<'de> Deserialize<'de>>(
        &self,
        dirname: &str,
        insert: impl Fn(&Self, T),
    ) {
        for value in self.read_json_dir(dirname) {
            insert(self, value);
        }
    }

    fn seed_defaults(&self) -> Result<(), AuthError> {
        self.ensure_person("alice", "Alice")?;
        self.ensure_person("bob", "Bob")?;
        self.ensure_org("loco")?;
        Ok(())
    }

    fn ensure_person(&self, handle: &str, name: &str) -> Result<(), AuthError> {
        if self.accounts.read().unwrap().contains_key(handle) {
            return Ok(());
        }
        match self.insert_person(
            handle,
            name,
            secret::hash_password(TEST_PASSWORD),
            None,
            |_| AuthError::UserAlreadyExists,
        ) {
            Ok(_) | Err(AuthError::UserAlreadyExists) => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn ensure_org(&self, handle: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.accounts_writer);
        if self.accounts.read().unwrap().contains_key(handle) {
            return Ok(());
        }
        let account = StoredAccount {
            handle: handle.to_string(),
            account_type: AccountType::Org,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let wrote = self.persist_account(&account)?;
        self.accounts
            .write()
            .unwrap()
            .insert(handle.to_string(), account);
        finish(wrote)
    }

    fn account_path(&self, handle: &str) -> PathBuf {
        self.base_dir
            .join("accounts")
            .join(format!("{handle}.json"))
    }

    fn identity_path(&self, handle: &str) -> PathBuf {
        self.base_dir
            .join("identities")
            .join(format!("{handle}.json"))
    }

    fn session_path(&self, token: &str) -> PathBuf {
        self.base_dir.join("sessions").join(format!("{token}.json"))
    }

    fn api_key_path(&self, id: &str) -> PathBuf {
        self.base_dir.join("api_keys").join(format!("{id}.json"))
    }

    fn org_member_path(&self, org: &str, handle: &str) -> PathBuf {
        self.base_dir
            .join("org_members")
            .join(org)
            .join(format!("{handle}.json"))
    }

    fn project_member_path(&self, project_id: &str, handle: &str) -> PathBuf {
        self.base_dir
            .join("project_members")
            .join(project_id)
            .join(format!("{handle}.json"))
    }

    fn persist_account(&self, account: &StoredAccount) -> Result<Wrote, AuthError> {
        write_json(&self.account_path(&account.handle), account)
    }

    fn persist_identity(&self, identity: &StoredIdentity) -> Result<Wrote, AuthError> {
        write_json(&self.identity_path(&identity.handle), identity)
    }

    fn delete_account_file(&self, handle: &str) -> Result<(), AuthError> {
        remove_json(&self.account_path(handle))
    }

    fn delete_identity_file(&self, handle: &str) -> Result<(), AuthError> {
        remove_json(&self.identity_path(handle))
    }

    fn persist_session(&self, session: &StoredSession) -> Result<Wrote, AuthError> {
        write_json(&self.session_path(&session.token), session)
    }

    fn delete_session_file(&self, token: &str) -> Result<(), AuthError> {
        remove_json(&self.session_path(token))
    }

    /// Drop a session from disk and then from the cache. A failed delete
    /// leaves the cache entry, so the next attempt sees the same session.
    fn forget_session(&self, token: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.sessions_writer);
        self.forget_session_locked(token)
    }

    fn forget_session_locked(&self, token: &str) -> Result<(), AuthError> {
        self.delete_session_file(token)?;
        self.sessions.write().unwrap().remove(token);
        Ok(())
    }

    /// Boot-time cleanup: expired sessions never come back, so there is no
    /// reason to carry them in memory or leave their files on disk.
    fn sweep_expired_sessions(&self, now: DateTime<Utc>) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.sessions_writer);
        let expired: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .values()
                .filter(|session| session.is_expired(now))
                .map(|session| session.token.clone())
                .collect()
        };
        for token in &expired {
            self.forget_session_locked(token)?;
        }
        Ok(())
    }

    fn persist_api_key(&self, key: &StoredApiKey) -> Result<Wrote, AuthError> {
        write_json(&self.api_key_path(&key.id), key)
    }

    fn persist_org_member(&self, member: &StoredOrgMember) -> Result<Wrote, AuthError> {
        write_json(&self.org_member_path(&member.org, &member.handle), member)
    }

    fn delete_org_member_file(&self, org: &str, handle: &str) -> Result<(), AuthError> {
        remove_json(&self.org_member_path(org, handle))
    }

    fn persist_project_member(&self, member: &StoredProjectMember) -> Result<Wrote, AuthError> {
        write_json(
            &self.project_member_path(&member.project, &member.handle),
            member,
        )
    }

    fn delete_project_member_file(&self, project_id: &str, handle: &str) -> Result<(), AuthError> {
        remove_json(&self.project_member_path(project_id, handle))
    }

    #[cfg(test)]
    fn wait_gates(slot: &Mutex<Option<std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)>>>) {
        let gates = slot.lock().unwrap_or_else(|err| err.into_inner()).clone();
        if let Some(gates) = gates {
            gates.0.wait();
            gates.1.wait();
        }
    }

    /// No-op unless a test installed barriers.
    #[cfg(test)]
    fn pause_before_write(&self) {
        Self::wait_gates(&self.pause_before_write);
    }

    #[cfg(test)]
    fn pause_after_account_delete(&self) {
        Self::wait_gates(&self.pause_after_account_delete);
    }

    #[cfg(not(test))]
    fn pause_before_write(&self) {}

    #[cfg(not(test))]
    fn pause_after_account_delete(&self) {}

    fn to_account(account: &StoredAccount) -> Account {
        Account {
            handle: account.handle.clone(),
            account_type: account.account_type,
            created_at: account.created_at.clone(),
        }
    }

    fn member_pending(&self, handle: &str) -> bool {
        !self.identities.read().unwrap().contains_key(handle)
    }

    fn account_type(&self, handle: &str) -> Option<AccountType> {
        self.accounts
            .read()
            .unwrap()
            .get(handle)
            .map(|a| a.account_type)
    }

    /// Charset of a missing member handle, with no length cap. `public` is
    /// reserved. A hyphen, a leading digit, or an empty name is
    /// [`member_handle_charset_sentence`]. Called only when the handle names
    /// no account: an account already on disk is accepted whatever its charset.
    fn reject_member_handle(handle: &str) -> Option<String> {
        if handle == PUBLIC_USERNAME {
            return Some(format!("handle name {handle:?} is reserved"));
        }
        if slug_charset_ok(handle) {
            return None;
        }
        Some(member_handle_charset_sentence(handle))
    }

    /// An account already on disk may be added, whatever its charset. A
    /// missing handle is 400 when illegal and 404 when well-formed.
    fn require_member_account(&self, handle: &str) -> Result<(), AuthError> {
        if self.account_type(handle).is_some() {
            return Ok(());
        }
        if let Some(msg) = Self::reject_member_handle(handle) {
            return Err(AuthError::InvalidHandle(msg));
        }
        Err(AuthError::UnknownAccount(handle.to_string()))
    }

    /// A handle that may be created: the slug charset, at most 63 characters
    /// (`http/names.rs`), and not the reserved name `public`. Called from
    /// `create_user`, `create_org` (the new org handle), and
    /// `auto_create_person`. Load, login, and member add do not call this.
    /// `auto_create_person` turns a rejection into `InvalidCredentials`, so
    /// login does not gain a handle oracle.
    fn is_valid_handle(handle: &str) -> bool {
        Self::reject_handle(handle).is_none()
    }

    /// `None` when `handle` may be created. `Some` is the 400 sentence.
    fn reject_handle(handle: &str) -> Option<String> {
        if handle == PUBLIC_USERNAME {
            return Some(format!("handle name {handle:?} is reserved"));
        }
        check_slug("handle", handle).err()
    }

    fn to_auth_user(identity: &StoredIdentity) -> AuthUser {
        AuthUser {
            id: identity.id.clone(),
            username: identity.handle.clone(),
            name: identity.name.clone(),
            account_type: AccountType::Person.as_str().to_string(),
            created_at: identity.created_at.clone(),
            last_login_at: identity.last_login_at.clone(),
        }
    }

    fn identity_by_id(&self, id: &str) -> Option<StoredIdentity> {
        self.identities
            .read()
            .unwrap()
            .values()
            .find(|i| i.id == id)
            .cloned()
    }

    fn auto_create_person(
        &self,
        handle: &str,
        password: Option<&str>,
        last_login_at: Option<String>,
    ) -> Result<StoredIdentity, AuthError> {
        if !Self::is_valid_handle(handle) {
            return Err(AuthError::InvalidCredentials);
        }
        let Some(password) = password.map(str::trim).filter(|s| !s.is_empty()) else {
            return Err(AuthError::InvalidCredentials);
        };
        // Hash before the accounts lock. Argon2 is slow, and the taken-name
        // check inside [`Self::insert_person`] is the one that counts.
        let password_hash = secret::hash_password(password);
        self.insert_person(
            handle,
            handle,
            password_hash,
            last_login_at,
            |account_type| match account_type {
                AccountType::Org => AuthError::InvalidCredentials,
                AccountType::Person => AuthError::UserAlreadyExists,
            },
        )
    }

    /// Check the name and insert a person under one `accounts` writer, the
    /// same critical section as [`AuthAdapter::create_org`].
    ///
    /// Two files and no transaction. The identity is written first and the
    /// account second, both while the accounts writer is held. A crash between
    /// them leaves `identities/{handle}.json` and no account file. That
    /// identity grants nothing: the taken-name check and project access key
    /// off the account, and the next create of the handle overwrites the
    /// identity. The reverse order would leave an account that owns
    /// `{handle}/*` and cannot log in.
    ///
    /// Lock order is the accounts writer, then the identities writer. Callers
    /// must not hold the identities writer and then take the accounts writer.
    /// The map write locks are taken only for the inserts.
    fn insert_person(
        &self,
        handle: &str,
        name: &str,
        password_hash: String,
        last_login_at: Option<String>,
        conflict: impl FnOnce(AccountType) -> AuthError,
    ) -> Result<StoredIdentity, AuthError> {
        let now = chrono::Utc::now().to_rfc3339();
        let account = StoredAccount {
            handle: handle.to_string(),
            account_type: AccountType::Person,
            created_at: now.clone(),
        };
        let identity = StoredIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            handle: handle.to_string(),
            name: name.to_string(),
            password_hash,
            created_at: now,
            last_login_at,
        };

        let _accounts = lock_writer(&self.accounts_writer);
        if let Some(existing) = self.accounts.read().unwrap().get(handle) {
            return Err(conflict(existing.account_type));
        }
        let _identities = lock_writer(&self.identities_writer);
        let identity_write = self.persist_identity(&identity)?;
        self.identities
            .write()
            .unwrap()
            .insert(handle.to_string(), identity.clone());
        let account_write = self.persist_account(&account)?;
        self.accounts
            .write()
            .unwrap()
            .insert(handle.to_string(), account);
        finish(identity_write)?;
        finish(account_write)?;
        Ok(identity)
    }

    /// Orgs this handle solely owns. Deleting them would leave the org
    /// with no owner.
    fn sole_owned_org(&self, handle: &str) -> Option<String> {
        let members = self.org_members.read().unwrap();
        let owned: Vec<String> = members
            .values()
            .filter(|m| m.handle == handle && m.role == OrgRole::Owner)
            .map(|m| m.org.clone())
            .collect();
        owned.into_iter().find(|org| {
            members
                .values()
                .filter(|m| m.org == *org && m.role == OrgRole::Owner)
                .count()
                == 1
        })
    }

    fn purge_memberships(&self, handle: &str) -> Result<(), AuthError> {
        {
            let _writer = lock_writer(&self.org_members_writer);
            let org_keys: Vec<(String, String)> = {
                let members = self.org_members.read().unwrap();
                members
                    .keys()
                    .filter(|(_, h)| h == handle)
                    .cloned()
                    .collect()
            };
            for key in &org_keys {
                self.delete_org_member_file(&key.0, &key.1)?;
                self.org_members.write().unwrap().remove(key);
            }
        }

        let _writer = lock_writer(&self.project_members_writer);
        let project_keys: Vec<(String, String)> = {
            let members = self.project_members.read().unwrap();
            members
                .keys()
                .filter(|(_, h)| h == handle)
                .cloned()
                .collect()
        };
        for key in &project_keys {
            self.delete_project_member_file(&key.0, &key.1)?;
            self.project_members.write().unwrap().remove(key);
        }
        Ok(())
    }
}

impl AuthAdapter for LocalAuthAdapter {
    fn login(&self, credentials: &LoginCredentials) -> Result<AuthSession, AuthError> {
        if credentials.username.is_empty() || credentials.username == PUBLIC_USERNAME {
            return Err(AuthError::InvalidCredentials);
        }

        let issued_at = Utc::now();
        let now = issued_at.to_rfc3339();

        let identity = {
            let account = self
                .accounts
                .read()
                .unwrap()
                .get(&credentials.username)
                .cloned();

            match account {
                Some(account) if account.account_type == AccountType::Org => {
                    return Err(AuthError::InvalidCredentials);
                }
                Some(_) => {
                    // Verify outside the writer mutex: argon2 is deliberately
                    // slow, and holding it would serialize every login.
                    let stored = self
                        .identities
                        .read()
                        .unwrap()
                        .get(&credentials.username)
                        .cloned()
                        .ok_or(AuthError::UserNotFound)?;
                    if !secret::verify_password(
                        &stored.password_hash,
                        credentials.password.as_deref(),
                    ) {
                        return Err(AuthError::InvalidCredentials);
                    }
                    // A delete or a rename can land during the verify. Re-read
                    // under the writer and keep that row; a missing row is the
                    // same `UserNotFound` the write lock used to return.
                    self.pause_before_write();
                    let _writer = lock_writer(&self.identities_writer);
                    let mut identity = self
                        .identities
                        .read()
                        .unwrap()
                        .get(&credentials.username)
                        .cloned()
                        .ok_or(AuthError::UserNotFound)?;
                    identity.last_login_at = Some(now.clone());
                    commit(self.persist_identity(&identity), || {
                        self.identities
                            .write()
                            .unwrap()
                            .insert(identity.handle.clone(), identity.clone());
                    })?;
                    identity
                }
                None => {
                    if !self.auto_create {
                        return Err(AuthError::InvalidCredentials);
                    }
                    self.auto_create_person(
                        &credentials.username,
                        credentials.password.as_deref(),
                        Some(now.clone()),
                    )?
                }
            }
        };

        let token = uuid::Uuid::new_v4().to_string();
        let session = StoredSession::new(token.clone(), identity.id.clone(), issued_at);
        let _sessions = lock_writer(&self.sessions_writer);
        commit(self.persist_session(&session), || {
            self.sessions
                .write()
                .unwrap()
                .insert(token.clone(), session.clone());
        })?;

        Ok(AuthSession {
            token,
            user: Self::to_auth_user(&identity),
        })
    }

    fn validate_session(&self, token: &str) -> Result<AuthSession, AuthError> {
        let session = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .get(token)
                .cloned()
                .ok_or(AuthError::SessionNotFound)?
        };
        if session.is_expired(Utc::now()) {
            self.forget_session(token)?;
            return Err(AuthError::SessionExpired);
        }
        let identity = self
            .identity_by_id(&session.identity_id)
            .ok_or(AuthError::UserNotFound)?;
        Ok(AuthSession {
            token: token.to_string(),
            user: Self::to_auth_user(&identity),
        })
    }

    fn logout(&self, token: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.sessions_writer);
        if !self.sessions.read().unwrap().contains_key(token) {
            return Err(AuthError::SessionNotFound);
        }
        self.delete_session_file(token)?;
        self.sessions.write().unwrap().remove(token);
        Ok(())
    }

    fn revoke_all_sessions(&self, identity_id: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.sessions_writer);
        let tokens: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .iter()
                .filter(|(_, s)| s.identity_id == identity_id)
                .map(|(token, _)| token.clone())
                .collect()
        };
        let mut first_err = None;
        for token in &tokens {
            match self.delete_session_file(token) {
                Ok(()) => {
                    self.sessions.write().unwrap().remove(token);
                }
                Err(err) => {
                    first_err.get_or_insert(err);
                }
            }
        }
        match first_err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn get_user(&self, user_id: &str) -> Result<Option<AuthUser>, AuthError> {
        Ok(self.identity_by_id(user_id).map(|i| Self::to_auth_user(&i)))
    }

    fn create_user(&self, req: &CreateUserRequest) -> Result<AuthUser, AuthError> {
        if let Some(msg) = Self::reject_handle(&req.username) {
            return Err(AuthError::InvalidHandle(msg));
        }
        let password = req.password.trim();
        if password.is_empty() {
            return Err(AuthError::InvalidCredentials);
        }
        let password_hash = secret::hash_password(password);
        let identity = self.insert_person(&req.username, &req.name, password_hash, None, |_| {
            AuthError::UserAlreadyExists
        })?;
        Ok(Self::to_auth_user(&identity))
    }

    fn update_user(
        &self,
        user_id: &str,
        updates: &UpdateUserRequest,
    ) -> Result<AuthUser, AuthError> {
        let _writer = lock_writer(&self.identities_writer);
        let identity = {
            let identities = self.identities.read().unwrap();
            let mut identity = identities
                .values()
                .find(|i| i.id == user_id)
                .cloned()
                .ok_or(AuthError::UserNotFound)?;
            if let Some(name) = &updates.name {
                identity.name = name.clone();
            }
            identity
        };
        commit(self.persist_identity(&identity), || {
            self.identities
                .write()
                .unwrap()
                .insert(identity.handle.clone(), identity.clone());
        })?;
        Ok(Self::to_auth_user(&identity))
    }

    fn delete_user(&self, user_id: &str) -> Result<(), AuthError> {
        let handle = self
            .identity_by_id(user_id)
            .map(|i| i.handle)
            .ok_or(AuthError::UserNotFound)?;
        if let Some(org) = self.sole_owned_org(&handle) {
            return Err(AuthError::SoleOrgOwner(org));
        }
        self.purge_memberships(&handle)?;
        self.revoke_all_sessions(user_id)?;
        // Account file first, and this writer stays held through the identity
        // delete (accounts → identities, same as `insert_person`). A crash
        // before the identity file is removed leaves an identity with no
        // account, which grants nothing. The reverse would leave an account
        // that owns `{handle}/*`. Releasing the accounts writer between the
        // two deletes would let a signup of this handle write a new identity,
        // and the identity delete would remove that person by handle.
        let _accounts = lock_writer(&self.accounts_writer);
        self.delete_account_file(&handle)?;
        self.accounts.write().unwrap().remove(&handle);
        self.pause_after_account_delete();
        let _identities = lock_writer(&self.identities_writer);
        self.delete_identity_file(&handle)?;
        self.identities.write().unwrap().remove(&handle);
        Ok(())
    }

    fn create_api_key(&self, identity_id: &str, label: &str) -> Result<ApiKey, AuthError> {
        if self.identity_by_id(identity_id).is_none() {
            return Err(AuthError::UserNotFound);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let key = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let stored = StoredApiKey {
            id: id.clone(),
            key_hash: secret::hash_api_key(&key),
            identity_id: identity_id.to_string(),
            label: label.to_string(),
            created_at: now.clone(),
            last_used_at: None,
            revoked: false,
        };
        let _writer = lock_writer(&self.api_keys_writer);
        commit(self.persist_api_key(&stored), || {
            self.api_keys
                .write()
                .unwrap()
                .insert(id.clone(), stored.clone());
        })?;
        Ok(ApiKey {
            id,
            key,
            label: label.to_string(),
            created_at: now,
        })
    }

    fn validate_api_key(&self, key: &str) -> Result<AuthSession, AuthError> {
        let hash = secret::hash_api_key(key);
        let api_keys = self.api_keys.read().unwrap();
        let stored = api_keys
            .values()
            .find(|k| k.key_hash == hash && !k.revoked)
            .ok_or(AuthError::InvalidCredentials)?;
        let identity = self
            .identity_by_id(&stored.identity_id)
            .ok_or(AuthError::UserNotFound)?;
        Ok(AuthSession {
            token: key.to_string(),
            user: Self::to_auth_user(&identity),
        })
    }

    fn revoke_api_key(&self, identity_id: &str, key_id: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.api_keys_writer);
        let key = {
            let api_keys = self.api_keys.read().unwrap();
            let key = api_keys
                .get(key_id)
                .ok_or(AuthError::Internal("api key not found".to_string()))?;
            if key.identity_id != identity_id {
                return Err(AuthError::Unauthorized);
            }
            let mut key = key.clone();
            key.revoked = true;
            key
        };
        commit(self.persist_api_key(&key), || {
            self.api_keys
                .write()
                .unwrap()
                .insert(key.id.clone(), key.clone());
        })?;
        Ok(())
    }

    fn list_api_keys(&self, identity_id: &str) -> Result<Vec<ApiKeyInfo>, AuthError> {
        let api_keys = self.api_keys.read().unwrap();
        Ok(api_keys
            .values()
            .filter(|k| k.identity_id == identity_id)
            .map(|k| ApiKeyInfo {
                id: k.id.clone(),
                label: k.label.clone(),
                created_at: k.created_at.clone(),
                last_used_at: k.last_used_at.clone(),
                revoked: k.revoked,
            })
            .collect())
    }

    fn get_account(&self, handle: &str) -> Result<Option<Account>, AuthError> {
        Ok(self
            .accounts
            .read()
            .unwrap()
            .get(handle)
            .map(Self::to_account))
    }

    fn create_org(&self, handle: &str, creator_handle: &str) -> Result<Account, AuthError> {
        if let Some(msg) = Self::reject_handle(handle) {
            return Err(AuthError::InvalidHandle(msg));
        }
        // The creator check, the taken-name check, and the account file share
        // this writer. The account file is written before the owner row. The
        // writer stays held through that row: `add_org_member` takes the org
        // members writer, not this one. A failed owner row removes the account
        // so the name can be retried. A crash between the two still leaves an
        // org with no owner.
        let _accounts = lock_writer(&self.accounts_writer);
        let creator_ready = self.accounts.read().unwrap().contains_key(creator_handle)
            && self.identities.read().unwrap().contains_key(creator_handle);
        if !creator_ready {
            return Err(AuthError::UserNotFound);
        }
        if self.accounts.read().unwrap().contains_key(handle) {
            return Err(AuthError::UserAlreadyExists);
        }
        let account = StoredAccount {
            handle: handle.to_string(),
            account_type: AccountType::Org,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let wrote = self.persist_account(&account)?;
        self.accounts
            .write()
            .unwrap()
            .insert(handle.to_string(), account.clone());
        let durability = finish(wrote);
        match self.add_org_member(handle, creator_handle, OrgRole::Owner) {
            Ok(_) | Err(AuthError::UserAlreadyExists) => {
                durability.map(|()| Self::to_account(&account))
            }
            Err(err) => match self.delete_account_file(handle) {
                Ok(()) => {
                    self.accounts.write().unwrap().remove(handle);
                    Err(err)
                }
                Err(remove_err) => Err(AuthError::Internal(format!(
                    "{err}; account {handle} was left in place: {remove_err}"
                ))),
            },
        }
    }

    fn org_role(&self, identity_handle: &str, account: &str) -> Result<Option<OrgRole>, AuthError> {
        Ok(self
            .org_members
            .read()
            .unwrap()
            .get(&(account.to_string(), identity_handle.to_string()))
            .map(|member| member.role))
    }

    fn project_access(
        &self,
        identity_handle: &str,
        project_id: &str,
    ) -> Result<Option<ProjectRole>, AuthError> {
        let Some((account, _)) = project_id.split_once('/') else {
            return Ok(None);
        };

        if self.org_role(identity_handle, account)? == Some(OrgRole::Owner) {
            return Ok(Some(ProjectRole::Developer));
        }

        if let Some(member) = self
            .project_members
            .read()
            .unwrap()
            .get(&(project_id.to_string(), identity_handle.to_string()))
        {
            return Ok(Some(member.role));
        }

        if self.account_type(account) == Some(AccountType::Person) && identity_handle == account {
            return Ok(Some(ProjectRole::Developer));
        }

        Ok(None)
    }

    fn add_project_member(
        &self,
        project_id: &str,
        handle: &str,
        role: ProjectRole,
    ) -> Result<ProjectMember, AuthError> {
        self.require_member_account(handle)?;
        if project_id.split_once('/').is_none() {
            return Err(AuthError::Internal(format!(
                "project id {project_id} is not account/name"
            )));
        }
        let _writer = lock_writer(&self.project_members_writer);
        let key = (project_id.to_string(), handle.to_string());
        if self.project_members.read().unwrap().contains_key(&key) {
            return Err(AuthError::UserAlreadyExists);
        }
        let member = StoredProjectMember {
            project: project_id.to_string(),
            handle: handle.to_string(),
            role,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        commit(self.persist_project_member(&member), || {
            self.project_members
                .write()
                .unwrap()
                .insert(key.clone(), member.clone());
        })?;
        Ok(ProjectMember {
            project: member.project,
            handle: member.handle,
            role: member.role,
            pending: self.member_pending(handle),
        })
    }

    fn update_project_member(
        &self,
        project_id: &str,
        handle: &str,
        role: ProjectRole,
    ) -> Result<ProjectMember, AuthError> {
        let _writer = lock_writer(&self.project_members_writer);
        let key = (project_id.to_string(), handle.to_string());
        let member = {
            let members = self.project_members.read().unwrap();
            let mut member = members.get(&key).cloned().ok_or(AuthError::UserNotFound)?;
            member.role = role;
            member
        };
        // Still holding the writer. A remove that runs here waits, instead of
        // deleting the row and having this snapshot write it back.
        self.pause_before_write();
        commit(self.persist_project_member(&member), || {
            self.project_members
                .write()
                .unwrap()
                .insert(key.clone(), member.clone());
        })?;
        Ok(ProjectMember {
            project: member.project,
            handle: member.handle,
            role: member.role,
            pending: self.member_pending(handle),
        })
    }

    fn remove_project_member(&self, project_id: &str, handle: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.project_members_writer);
        let key = (project_id.to_string(), handle.to_string());
        if !self.project_members.read().unwrap().contains_key(&key) {
            return Err(AuthError::UserNotFound);
        }
        self.delete_project_member_file(project_id, handle)?;
        self.project_members.write().unwrap().remove(&key);
        Ok(())
    }

    fn list_project_members(&self, project_id: &str) -> Result<Vec<ProjectMember>, AuthError> {
        let members = self.project_members.read().unwrap();
        Ok(members
            .values()
            .filter(|m| m.project == project_id)
            .map(|m| ProjectMember {
                project: m.project.clone(),
                handle: m.handle.clone(),
                role: m.role,
                pending: self.member_pending(&m.handle),
            })
            .collect())
    }

    fn add_org_member(
        &self,
        org: &str,
        handle: &str,
        role: OrgRole,
    ) -> Result<OrgMember, AuthError> {
        self.require_member_account(handle)?;
        match self.account_type(org) {
            Some(AccountType::Org) => {}
            Some(AccountType::Person) => return Err(AuthError::Unauthorized),
            None => return Err(AuthError::UserNotFound),
        }
        let _writer = lock_writer(&self.org_members_writer);
        let key = (org.to_string(), handle.to_string());
        if self.org_members.read().unwrap().contains_key(&key) {
            return Err(AuthError::UserAlreadyExists);
        }
        let member = StoredOrgMember {
            org: org.to_string(),
            handle: handle.to_string(),
            role,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        commit(self.persist_org_member(&member), || {
            self.org_members
                .write()
                .unwrap()
                .insert(key.clone(), member.clone());
        })?;
        Ok(OrgMember {
            org: member.org,
            handle: member.handle,
            role: member.role,
            pending: self.member_pending(handle),
        })
    }

    fn remove_org_member(&self, org: &str, handle: &str) -> Result<(), AuthError> {
        let _writer = lock_writer(&self.org_members_writer);
        let key = (org.to_string(), handle.to_string());
        if !self.org_members.read().unwrap().contains_key(&key) {
            return Err(AuthError::UserNotFound);
        }
        self.delete_org_member_file(org, handle)?;
        self.org_members.write().unwrap().remove(&key);
        Ok(())
    }

    fn list_org_members(&self, org: &str) -> Result<Vec<OrgMember>, AuthError> {
        let members = self.org_members.read().unwrap();
        Ok(members
            .values()
            .filter(|m| m.org == org)
            .map(|m| OrgMember {
                org: m.org.clone(),
                handle: m.handle.clone(),
                role: m.role,
                pending: self.member_pending(&m.handle),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthAdapter;

    fn adapter() -> (tempfile::TempDir, LocalAuthAdapter) {
        let dir = tempfile::TempDir::new().unwrap();
        let adapter = LocalAuthAdapter::new(dir.path(), true);
        (dir, adapter)
    }

    /// Production default: auto-create off. [`adapter`] passes `true`.
    fn adapter_without_auto_create() -> (tempfile::TempDir, LocalAuthAdapter) {
        let dir = tempfile::TempDir::new().unwrap();
        let adapter = LocalAuthAdapter::new(dir.path(), false);
        (dir, adapter)
    }

    fn login(adapter: &LocalAuthAdapter, username: &str, password: Option<&str>) -> AuthSession {
        adapter
            .login(&LoginCredentials {
                username: username.to_string(),
                password: password.map(str::to_string),
            })
            .unwrap()
    }

    fn login_ok(adapter: &LocalAuthAdapter, username: &str) -> AuthSession {
        login(adapter, username, Some(TEST_PASSWORD))
    }

    #[test]
    fn seed_creates_alice_bob_persons_and_loco_org() {
        let (_dir, adapter) = adapter();
        let accounts = adapter.accounts.read().unwrap();
        assert_eq!(
            accounts.get("alice").unwrap().account_type,
            AccountType::Person
        );
        assert_eq!(
            accounts.get("bob").unwrap().account_type,
            AccountType::Person
        );
        assert_eq!(accounts.get("loco").unwrap().account_type, AccountType::Org);
        assert!(adapter.identities.read().unwrap().contains_key("alice"));
        assert!(adapter.identities.read().unwrap().contains_key("bob"));
        assert!(!adapter.identities.read().unwrap().contains_key("loco"));
    }

    #[test]
    fn login_does_not_need_a_site() {
        let (_dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
        assert_eq!(session.user.username, "alice");
        assert_eq!(session.user.account_type, "person");
        assert!(!session.token.is_empty());
        let json = serde_json::to_value(&session.user).unwrap();
        assert!(json.get("site_id").is_none());
    }

    #[test]
    fn login_with_correct_password_succeeds() {
        let (_dir, adapter) = adapter();
        let session = login(&adapter, "alice", Some(TEST_PASSWORD));
        assert_eq!(session.user.username, "alice");
    }

    #[test]
    fn login_with_wrong_password_fails() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "alice".to_string(),
                password: Some("nope".to_string()),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_as_org_fails() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "loco".to_string(),
                password: Some(TEST_PASSWORD.to_string()),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_without_password_fails() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "alice".to_string(),
                password: None,
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_unknown_handle_creates_person_when_auto_create_on() {
        let dir = tempfile::TempDir::new().unwrap();
        let adapter = LocalAuthAdapter::new(dir.path(), true);
        let session = login_ok(&adapter, "testuser");
        assert_eq!(session.user.username, "testuser");
        assert_eq!(session.user.account_type, "person");
        let accounts = adapter.accounts.read().unwrap();
        assert_eq!(
            accounts.get("testuser").unwrap().account_type,
            AccountType::Person
        );
    }

    #[test]
    fn session_hangs_off_identity_and_reloads() {
        let (dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
        let token = session.token.clone();
        let identity_id = session.user.id.clone();

        drop(adapter);
        let reloaded = LocalAuthAdapter::new(dir.path(), true);
        let validated = reloaded.validate_session(&token).unwrap();
        assert_eq!(validated.user.id, identity_id);
        assert_eq!(validated.user.username, "alice");
        assert!(serde_json::to_value(&validated.user)
            .unwrap()
            .get("site_id")
            .is_none());
    }

    /// Rewrite a live session so it looks like it was issued `days` ago —
    /// cheaper than waiting out a real TTL.
    fn backdate_session(adapter: &LocalAuthAdapter, token: &str, days: i64) {
        let mut session = adapter
            .sessions
            .read()
            .unwrap()
            .get(token)
            .cloned()
            .expect("session is live");
        let issued_at = Utc::now() - Duration::days(days);
        session.created_at = issued_at.to_rfc3339();
        session.expires_at = Some((issued_at + Duration::days(SESSION_TTL_DAYS)).to_rfc3339());
        let wrote = adapter.persist_session(&session).unwrap();
        adapter
            .sessions
            .write()
            .unwrap()
            .insert(token.to_string(), session);
        super::finish(wrote).unwrap();
    }

    fn session_file(dir: &Path, token: &str) -> PathBuf {
        dir.join("sessions").join(format!("{token}.json"))
    }

    #[test]
    fn session_past_ttl_is_rejected_and_forgotten() {
        let (dir, adapter) = adapter();
        let token = login_ok(&adapter, "alice").token;
        backdate_session(&adapter, &token, SESSION_TTL_DAYS + 1);

        let err = adapter.validate_session(&token).unwrap_err();
        assert!(matches!(err, AuthError::SessionExpired));

        // Rejecting it also drops it, so it cannot be retried and cannot keep
        // occupying the cache or the disk.
        assert!(!adapter.sessions.read().unwrap().contains_key(&token));
        assert!(!session_file(dir.path(), &token).exists());
        assert!(matches!(
            adapter.validate_session(&token).unwrap_err(),
            AuthError::SessionNotFound
        ));
    }

    #[test]
    fn session_within_ttl_still_validates() {
        let (_dir, adapter) = adapter();
        let token = login_ok(&adapter, "alice").token;
        backdate_session(&adapter, &token, SESSION_TTL_DAYS - 1);

        let validated = adapter.validate_session(&token).unwrap();
        assert_eq!(validated.user.username, "alice");
    }

    #[test]
    fn expired_sessions_are_swept_on_load() {
        let (dir, adapter) = adapter();
        let fresh = login_ok(&adapter, "alice").token;
        let stale = login_ok(&adapter, "bob").token;
        backdate_session(&adapter, &stale, SESSION_TTL_DAYS + 1);

        drop(adapter);
        let reloaded = LocalAuthAdapter::new(dir.path(), true);

        assert!(!reloaded.sessions.read().unwrap().contains_key(&stale));
        assert!(!session_file(dir.path(), &stale).exists());
        assert_eq!(
            reloaded.validate_session(&fresh).unwrap().user.username,
            "alice"
        );
    }

    /// Session files written before expiry landed carry only `created_at`.
    #[test]
    fn session_file_without_expires_at_ages_out_from_created_at() {
        let (dir, adapter) = adapter();
        let identity_id = login_ok(&adapter, "alice").user.id;
        drop(adapter);

        let write_legacy = |token: &str, age_days: i64| {
            let created_at = (Utc::now() - Duration::days(age_days)).to_rfc3339();
            std::fs::write(
                session_file(dir.path(), token),
                serde_json::json!({
                    "token": token,
                    "identity_id": identity_id,
                    "created_at": created_at,
                })
                .to_string(),
            )
            .unwrap();
        };
        write_legacy("legacy-fresh", 1);
        write_legacy("legacy-stale", SESSION_TTL_DAYS + 1);

        let adapter = LocalAuthAdapter::new(dir.path(), true);
        assert_eq!(
            adapter
                .validate_session("legacy-fresh")
                .unwrap()
                .user
                .username,
            "alice"
        );
        assert!(!session_file(dir.path(), "legacy-stale").exists());
    }

    /// An undateable session is not a session we can vouch for.
    #[test]
    fn session_with_unparseable_dates_is_expired() {
        let session = StoredSession {
            token: "t".to_string(),
            identity_id: "i".to_string(),
            created_at: "whenever".to_string(),
            expires_at: None,
        };
        assert!(session.is_expired(Utc::now()));
        assert!(StoredSession {
            expires_at: Some("whenever".to_string()),
            ..session
        }
        .is_expired(Utc::now()));
    }

    #[test]
    fn api_key_hangs_off_identity() {
        let (_dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
        let key = adapter.create_api_key(&session.user.id, "ci").unwrap();
        let via_key = adapter.validate_api_key(&key.key).unwrap();
        assert_eq!(via_key.user.username, "alice");
        assert_eq!(via_key.user.id, session.user.id);

        adapter.revoke_api_key(&session.user.id, &key.id).unwrap();
        assert!(adapter.validate_api_key(&key.key).is_err());
    }

    #[test]
    fn api_key_file_stores_a_digest_not_the_key() {
        let (dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
        let key = adapter.create_api_key(&session.user.id, "ci").unwrap();

        let path = dir.path().join("api_keys").join(format!("{}.json", key.id));
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            !contents.contains(&key.key),
            "api key file holds the bearer token: {contents}"
        );
        let stored: StoredApiKey = serde_json::from_str(&contents).unwrap();
        assert_eq!(stored.key_hash, secret::hash_api_key(&key.key));
    }

    /// Files written before hashing landed hold the password itself. Loading
    /// them upgrades the file in place — the login still works, and the
    /// plaintext stops living on disk.
    #[test]
    fn legacy_plaintext_identity_is_rehashed_on_load() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            let adapter = LocalAuthAdapter::new(dir.path(), true);
            let identity = adapter.identities.read().unwrap()["alice"].clone();
            let legacy = serde_json::json!({
                "id": identity.id,
                "handle": "alice",
                "name": "Alice",
                "password": TEST_PASSWORD,
                "created_at": identity.created_at,
                "last_login_at": null,
            });
            std::fs::write(
                dir.path().join("identities/alice.json"),
                serde_json::to_string_pretty(&legacy).unwrap(),
            )
            .unwrap();
        }

        let adapter = LocalAuthAdapter::new(dir.path(), true);
        assert_eq!(login_ok(&adapter, "alice").user.username, "alice");
        assert!(adapter
            .login(&LoginCredentials {
                username: "alice".to_string(),
                password: Some("nope".to_string()),
            })
            .is_err());

        let contents = std::fs::read_to_string(dir.path().join("identities/alice.json")).unwrap();
        assert!(
            !contents.contains(&format!("\"{TEST_PASSWORD}\"")),
            "identity file still holds the plaintext password: {contents}"
        );
        let stored: StoredIdentity = serde_json::from_str(&contents).unwrap();
        assert!(secret::is_password_hash(&stored.password_hash));
    }

    #[test]
    fn legacy_plaintext_api_key_is_rehashed_on_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let raw_key = {
            let adapter = LocalAuthAdapter::new(dir.path(), true);
            let session = login_ok(&adapter, "alice");
            let key = adapter.create_api_key(&session.user.id, "ci").unwrap();
            let mut stored = adapter.api_keys.read().unwrap()[&key.id].clone();
            stored.key_hash = key.key.clone();
            // The file is what the next load reads. Finish surfaces NotDurable
            // without removing the bytes the reload will hash.
            super::finish(adapter.persist_api_key(&stored).unwrap()).unwrap();
            key.key
        };

        let adapter = LocalAuthAdapter::new(dir.path(), true);
        assert_eq!(
            adapter.validate_api_key(&raw_key).unwrap().user.username,
            "alice"
        );
        for entry in std::fs::read_dir(dir.path().join("api_keys")).unwrap() {
            let contents = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(
                !contents.contains(&raw_key),
                "api key file still holds the bearer token: {contents}"
            );
        }
    }

    #[test]
    fn public_session_is_not_site_scoped() {
        let session = AuthSession::public();
        assert_eq!(session.user.username, PUBLIC_USERNAME);
        let json = serde_json::to_value(&session.user).unwrap();
        assert!(json.get("site_id").is_none());
    }

    #[test]
    fn person_is_implicit_developer_of_own_projects() {
        let (_dir, adapter) = adapter();
        assert_eq!(
            adapter.project_access("alice", "alice/testapp").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(
            adapter.project_access("bob", "alice/testapp").unwrap(),
            None
        );
    }

    #[test]
    fn org_owner_is_developer_on_every_org_project() {
        let (_dir, adapter) = adapter();
        adapter.create_org("acme", "alice").unwrap();
        assert_eq!(
            adapter.project_access("alice", "acme/crm").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(adapter.project_access("bob", "acme/crm").unwrap(), None);
    }

    /// A project named `_` is a real project. Membership on it is a project
    /// role, and the org role is the org row alone.
    #[test]
    fn org_role_is_independent_of_a_project_named_underscore() {
        let (_dir, adapter) = adapter();
        adapter.create_org("acme", "alice").unwrap();
        adapter
            .add_org_member("acme", "bob", OrgRole::Member)
            .unwrap();
        adapter
            .create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol".to_string(),
                password: TEST_PASSWORD.to_string(),
            })
            .unwrap();
        adapter
            .add_project_member("acme/_", "carol", ProjectRole::Developer)
            .unwrap();

        assert_eq!(
            adapter.org_role("alice", "acme").unwrap(),
            Some(OrgRole::Owner)
        );
        assert_eq!(
            adapter.org_role("bob", "acme").unwrap(),
            Some(OrgRole::Member)
        );
        assert_eq!(adapter.org_role("carol", "acme").unwrap(), None);
        assert_eq!(adapter.org_role("alice", "missing").unwrap(), None);

        assert_eq!(
            adapter.project_access("carol", "acme/_").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(adapter.project_access("carol", "acme/crm").unwrap(), None);
        assert_eq!(
            adapter.project_access("alice", "acme/_").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(adapter.project_access("bob", "acme/_").unwrap(), None);
    }

    #[test]
    fn project_editor_cannot_develop() {
        let (_dir, adapter) = adapter();
        adapter
            .add_project_member("alice/testapp", "bob", ProjectRole::Editor)
            .unwrap();
        assert_eq!(
            adapter.project_access("bob", "alice/testapp").unwrap(),
            Some(ProjectRole::Editor)
        );
        assert!(!adapter
            .project_access("bob", "alice/testapp")
            .unwrap()
            .unwrap()
            .can_develop());
        assert!(adapter
            .project_access("bob", "alice/testapp")
            .unwrap()
            .unwrap()
            .can_edit_data());
    }

    #[test]
    fn invite_unknown_handle_is_not_a_member() {
        let (_dir, adapter) = adapter();
        adapter.create_org("acme", "alice").unwrap();

        let missing = adapter
            .add_project_member("alice/testapp", "carol", ProjectRole::Editor)
            .unwrap_err();
        assert!(matches!(missing, AuthError::UnknownAccount(handle) if handle == "carol"));
        assert!(adapter
            .list_project_members("alice/testapp")
            .unwrap()
            .is_empty());

        let illegal = adapter
            .add_project_member("alice/testapp", "bad-handle", ProjectRole::Editor)
            .unwrap_err();
        assert!(
            matches!(illegal, AuthError::InvalidHandle(msg) if msg.contains("bad-handle") && !msg.contains("1-63"))
        );

        let long = "c".repeat(64);
        let unknown_long = adapter
            .add_org_member("acme", &long, OrgRole::Member)
            .unwrap_err();
        assert!(matches!(unknown_long, AuthError::UnknownAccount(handle) if handle == long));

        let reserved = adapter
            .add_org_member("acme", PUBLIC_USERNAME, OrgRole::Member)
            .unwrap_err();
        assert!(matches!(reserved, AuthError::InvalidHandle(msg) if msg.contains("reserved")));

        let member = adapter
            .add_project_member("alice/testapp", "bob", ProjectRole::Editor)
            .unwrap();
        assert!(!member.pending);
    }

    fn write_legacy_person(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join("accounts")).unwrap();
        std::fs::create_dir_all(dir.join("identities")).unwrap();
        std::fs::write(
            dir.join("accounts/lego-reseller.json"),
            r#"{"handle":"lego-reseller","type":"person","created_at":"2020-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("identities/lego-reseller.json"),
            r#"{"id":"legacy","handle":"lego-reseller","name":"Legacy","password":"password","created_at":"2020-01-01T00:00:00Z","last_login_at":null}"#,
        )
        .unwrap();
    }

    #[test]
    fn legacy_account_can_be_a_member() {
        let dir = tempfile::TempDir::new().unwrap();
        write_legacy_person(dir.path());
        let adapter = LocalAuthAdapter::new(dir.path(), false);

        let project = adapter
            .add_project_member("alice/shop", "lego-reseller", ProjectRole::Developer)
            .unwrap();
        assert_eq!(project.handle, "lego-reseller");
        assert!(!project.pending);

        adapter.accounts.write().unwrap().insert(
            "old-org".to_string(),
            StoredAccount {
                handle: "old-org".to_string(),
                account_type: AccountType::Org,
                created_at: "2020-01-01T00:00:00Z".to_string(),
            },
        );
        let org = adapter
            .add_org_member("old-org", "lego-reseller", OrgRole::Member)
            .unwrap();
        assert_eq!(org.org, "old-org");
        assert_eq!(org.handle, "lego-reseller");
        assert!(!org.pending);
    }

    #[test]
    fn create_org_rejects_illegal_handle_and_stores_nothing() {
        let (dir, adapter) = adapter();
        let err = adapter.create_org("my-org", "alice").unwrap_err();
        assert!(
            matches!(err, AuthError::InvalidHandle(msg) if msg.contains("my-org") && msg.contains("1-63"))
        );
        assert!(!adapter.accounts.read().unwrap().contains_key("my-org"));
        assert!(!dir.path().join("accounts/my-org.json").exists());
    }

    #[test]
    fn legacy_creator_owns_the_org() {
        let dir = tempfile::TempDir::new().unwrap();
        write_legacy_person(dir.path());
        let adapter = LocalAuthAdapter::new(dir.path(), false);

        let org = adapter.create_org("acme", "lego-reseller").unwrap();
        assert_eq!(org.handle, "acme");
        assert_eq!(org.account_type, AccountType::Org);
        assert!(adapter.accounts.read().unwrap().contains_key("acme"));
        assert!(dir.path().join("accounts/acme.json").exists());
        assert_eq!(
            adapter.org_role("lego-reseller", "acme").unwrap(),
            Some(OrgRole::Owner)
        );
        assert!(adapter
            .accounts
            .read()
            .unwrap()
            .contains_key("lego-reseller"));
    }

    #[test]
    fn create_org_missing_creator_stores_nothing() {
        let (dir, adapter) = adapter();
        let missing = adapter.create_org("acme", "carol").unwrap_err();
        assert!(matches!(missing, AuthError::UserNotFound));
        assert!(!adapter.accounts.read().unwrap().contains_key("acme"));
        assert!(!dir.path().join("accounts/acme.json").exists());

        // An org account has no identity, so it cannot be recorded as owner.
        let org_creator = adapter.create_org("shoporg", "loco").unwrap_err();
        assert!(matches!(org_creator, AuthError::UserNotFound));
        assert!(!adapter.accounts.read().unwrap().contains_key("shoporg"));
        assert!(!dir.path().join("accounts/shoporg.json").exists());
    }

    fn assert_invalid_credentials(
        adapter: &LocalAuthAdapter,
        username: &str,
        password: Option<&str>,
    ) {
        let err = adapter
            .login(&LoginCredentials {
                username: username.to_string(),
                password: password.map(str::to_string),
            })
            .unwrap_err();
        let message = err.to_string();
        assert_eq!(message, "invalid credentials", "{username}");
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_failures_are_one_error() {
        let (_dir, off) = adapter_without_auto_create();
        assert_invalid_credentials(&off, "alice", Some("nope"));
        assert_invalid_credentials(&off, "nobody", Some(TEST_PASSWORD));
        assert_invalid_credentials(&off, "loco", Some(TEST_PASSWORD));
        assert_invalid_credentials(&off, "lego-reseller", Some(TEST_PASSWORD));
        assert!(!off.accounts.read().unwrap().contains_key("nobody"));
        assert!(!off.accounts.read().unwrap().contains_key("lego-reseller"));

        // Auto-create still refuses an illegal handle as invalid credentials
        // and does not store it. An org handle is the same refusal.
        let (_dir, on) = adapter();
        assert_invalid_credentials(&on, "lego-reseller", Some(TEST_PASSWORD));
        assert!(!on.accounts.read().unwrap().contains_key("lego-reseller"));
        assert_invalid_credentials(&on, "loco", Some(TEST_PASSWORD));
    }

    #[test]
    fn create_org_rejects_taken_handle() {
        let (_dir, adapter) = adapter();
        let err = adapter.create_org("alice", "bob").unwrap_err();
        assert!(matches!(err, AuthError::UserAlreadyExists));
    }

    #[test]
    fn login_unknown_handle_without_auto_create_squats_nothing() {
        let (dir, adapter) = adapter_without_auto_create();
        let err = adapter
            .login(&LoginCredentials {
                username: "acme".to_string(),
                password: Some(TEST_PASSWORD.to_string()),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
        assert!(!adapter.accounts.read().unwrap().contains_key("acme"));
        assert!(!adapter.identities.read().unwrap().contains_key("acme"));
        assert!(!dir.path().join("identities/acme.json").exists());
        assert!(!dir.path().join("accounts/acme.json").exists());
        assert_eq!(adapter.project_access("acme", "acme/crm").unwrap(), None);
    }

    #[test]
    fn seeded_login_still_works_without_auto_create() {
        let (_dir, adapter) = adapter_without_auto_create();
        let session = login_ok(&adapter, "alice");
        assert_eq!(session.user.username, "alice");
    }

    #[test]
    fn auto_create_rejects_missing_password() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "newbie".to_string(),
                password: None,
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
        assert!(!adapter.identities.read().unwrap().contains_key("newbie"));
    }

    #[test]
    fn create_user_rejects_empty_password() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .create_user(&CreateUserRequest {
                username: "newbie".to_string(),
                name: "Newbie".to_string(),
                password: "  ".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn delete_user_purges_memberships_so_handle_reuse_does_not_inherit() {
        let (_dir, adapter) = adapter();
        let bob = login_ok(&adapter, "bob");
        adapter.create_org("acme", "alice").unwrap();
        adapter
            .add_org_member("acme", "bob", OrgRole::Member)
            .unwrap();
        adapter
            .add_project_member("alice/testapp", "bob", ProjectRole::Editor)
            .unwrap();

        adapter.delete_user(&bob.user.id).unwrap();
        assert!(adapter
            .list_org_members("acme")
            .unwrap()
            .iter()
            .all(|m| m.handle != "bob"));
        assert!(adapter
            .list_project_members("alice/testapp")
            .unwrap()
            .iter()
            .all(|m| m.handle != "bob"));

        adapter
            .create_user(&CreateUserRequest {
                username: "bob".to_string(),
                name: "Bob".to_string(),
                password: TEST_PASSWORD.to_string(),
            })
            .unwrap();
        assert_eq!(
            adapter.project_access("bob", "alice/testapp").unwrap(),
            None
        );
        assert!(adapter
            .list_org_members("acme")
            .unwrap()
            .iter()
            .all(|m| m.handle != "bob"));
    }

    #[test]
    fn delete_user_refuses_sole_org_owner() {
        let (_dir, adapter) = adapter();
        let alice = login_ok(&adapter, "alice");
        adapter.create_org("acme", "alice").unwrap();
        let err = adapter.delete_user(&alice.user.id).unwrap_err();
        assert!(matches!(err, AuthError::SoleOrgOwner(org) if org == "acme"));
        assert!(adapter.identities.read().unwrap().contains_key("alice"));

        adapter
            .add_org_member("acme", "bob", OrgRole::Owner)
            .unwrap();
        adapter.delete_user(&alice.user.id).unwrap();
        assert!(!adapter.identities.read().unwrap().contains_key("alice"));
        assert!(adapter
            .list_org_members("acme")
            .unwrap()
            .iter()
            .any(|m| m.handle == "bob" && m.role == OrgRole::Owner));
    }

    /// Directory mode is what stops a persist. Root ignores it, so the test
    /// refuses to pass vacuously there.
    struct Unlock(PathBuf);

    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    fn freeze(dir: &Path) -> Unlock {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let probe = dir.join(".loco-probe");
        if std::fs::write(&probe, b"x").is_ok() {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_file(&probe);
            panic!("directory mode did not block writes; this test needs a non-root user");
        }
        Unlock(dir.to_path_buf())
    }

    #[test]
    fn persist_into_unwritable_directory_surfaces_error() {
        let (dir, adapter) = adapter_without_auto_create();
        let leaf = dir.path().join("project_members/alice/shop");
        let _unlock = freeze(&leaf);

        let err = adapter
            .add_project_member("alice/shop", "bob", ProjectRole::Editor)
            .unwrap_err();
        assert!(matches!(&err, AuthError::Internal(message) if !message.is_empty()));
        let response = crate::auth::auth_error_to_response(err);
        assert_eq!(response.status().as_u16(), 500);

        let key = ("alice/shop".to_string(), "bob".to_string());
        assert!(!adapter.project_members.read().unwrap().contains_key(&key));
        assert!(!leaf.join("bob.json").exists());
    }

    /// Identity is written first. An account directory that rejects the second
    /// write leaves the identity on disk and in the cache, and no account.
    #[test]
    fn person_create_keeps_identity_when_account_write_fails() {
        let (dir, adapter) = adapter_without_auto_create();
        let accounts = dir.path().join("accounts");
        let _unlock = freeze(&accounts);

        let err = adapter
            .create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol".to_string(),
                password: "secret".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::Internal(_)));
        assert!(!adapter.accounts.read().unwrap().contains_key("carol"));
        assert!(adapter.identities.read().unwrap().contains_key("carol"));
        assert!(dir.path().join("identities/carol.json").exists());
        assert!(!accounts.join("carol.json").exists());
    }

    #[test]
    fn load_skips_temp_siblings() {
        let (dir, adapter) = adapter_without_auto_create();
        drop(adapter);

        std::fs::write(
            dir.path().join("accounts/.loco-write-1-2-3.json"),
            r#"{"handle":"ghost","type":"person","created_at":"2020-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("accounts/.loco-write-1-2-3"), "not json").unwrap();

        let org_dir = dir.path().join("org_members/acme");
        std::fs::create_dir_all(&org_dir).unwrap();
        std::fs::write(
            org_dir.join(".loco-write-9.json"),
            r#"{"org":"acme","handle":"ghost","role":"owner","created_at":"2020-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let reloaded = LocalAuthAdapter::new(dir.path(), false);
        assert!(!reloaded.accounts.read().unwrap().contains_key("ghost"));
        assert!(reloaded.accounts.read().unwrap().contains_key("alice"));
        assert!(reloaded.list_org_members("acme").unwrap().is_empty());
    }

    #[test]
    fn person_create_leaves_no_temp_file() {
        let (dir, adapter) = adapter_without_auto_create();
        adapter
            .create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol".to_string(),
                password: "secret".to_string(),
            })
            .unwrap();
        for dirname in ["accounts", "identities"] {
            for entry in std::fs::read_dir(dir.path().join(dirname)).unwrap() {
                let name = entry.unwrap().file_name();
                let name = name.to_string_lossy();
                assert!(!name.starts_with(".loco-"), "{name}");
            }
        }
    }

    /// Two barriers. The caller waits on the first at its pause; the test
    /// thread does the racing work, then both pass the second.
    fn install_gates(
        slot: &Mutex<Option<std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)>>>,
    ) -> std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)> {
        let gates = std::sync::Arc::new((std::sync::Barrier::new(2), std::sync::Barrier::new(2)));
        *slot.lock().unwrap() = Some(std::sync::Arc::clone(&gates));
        gates
    }

    fn install_pause(
        adapter: &LocalAuthAdapter,
    ) -> std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)> {
        install_gates(&adapter.pause_before_write)
    }

    fn install_pause_after_account_delete(
        adapter: &LocalAuthAdapter,
    ) -> std::sync::Arc<(std::sync::Barrier, std::sync::Barrier)> {
        install_gates(&adapter.pause_after_account_delete)
    }

    #[test]
    fn login_does_not_resurrect_a_deleted_user() {
        use std::sync::Arc;
        use std::thread;

        let (dir, adapter) = adapter_without_auto_create();
        let adapter = Arc::new(adapter);
        let user = adapter
            .create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol".to_string(),
                password: "secret".to_string(),
            })
            .unwrap();
        let gates = install_pause(&adapter);
        let logging_in = Arc::clone(&adapter);
        let join = thread::spawn(move || {
            logging_in.login(&LoginCredentials {
                username: "carol".to_string(),
                password: Some("secret".to_string()),
            })
        });
        gates.0.wait();
        adapter.delete_user(&user.id).unwrap();
        gates.1.wait();

        let err = join.join().expect("login thread").unwrap_err();
        assert!(matches!(err, AuthError::UserNotFound));
        assert!(!adapter.identities.read().unwrap().contains_key("carol"));
        assert!(!adapter.accounts.read().unwrap().contains_key("carol"));
        assert!(adapter.sessions.read().unwrap().is_empty());
        assert!(!dir.path().join("identities/carol.json").exists());
        assert!(!dir.path().join("accounts/carol.json").exists());
    }

    /// Signup of the deleted handle, landing after the account file is gone
    /// and before the identity file is removed. The accounts writer is held
    /// across both, so the signup cannot write until the old identity is
    /// gone. If that writer is released in between, the signup commits and
    /// the identity delete removes the new person by handle.
    #[test]
    fn delete_does_not_drop_a_signup_of_the_same_handle() {
        use std::sync::Arc;
        use std::thread;

        let (dir, adapter) = adapter_without_auto_create();
        let adapter = Arc::new(adapter);
        let user = adapter
            .create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol".to_string(),
                password: "secret".to_string(),
            })
            .unwrap();
        let old_id = user.id.clone();
        let gates = install_pause_after_account_delete(&adapter);
        let deleting = Arc::clone(&adapter);
        let user_id = user.id;
        let delete_thread = thread::spawn(move || deleting.delete_user(&user_id));
        gates.0.wait();

        let accounts_held = adapter.accounts_writer.try_lock().is_err();
        let signing_up = Arc::clone(&adapter);
        let signup = thread::spawn(move || {
            signing_up.create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol Again".to_string(),
                password: "other-secret".to_string(),
            })
        });
        // Signup takes the accounts writer before it writes. When delete
        // still holds that writer, signup has to run after delete finishes.
        // When delete has released it, finish the signup first so the
        // identity delete removes the new person and the assertion fails.
        let new_user = if accounts_held {
            gates.1.wait();
            delete_thread.join().expect("delete thread").unwrap();
            signup.join().expect("signup thread").unwrap()
        } else {
            let created = signup.join().expect("signup thread").unwrap();
            gates.1.wait();
            delete_thread.join().expect("delete thread").unwrap();
            created
        };

        assert_ne!(new_user.id, old_id);
        let identity = adapter
            .identities
            .read()
            .unwrap()
            .get("carol")
            .cloned()
            .expect("signup identity");
        assert_eq!(identity.id, new_user.id);
        assert_eq!(identity.name, "Carol Again");
        let on_disk: StoredIdentity = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("identities/carol.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(on_disk.id, new_user.id);
        assert!(dir.path().join("accounts/carol.json").exists());
        let session = adapter
            .login(&LoginCredentials {
                username: "carol".to_string(),
                password: Some("other-secret".to_string()),
            })
            .unwrap();
        assert_eq!(session.user.id, new_user.id);
    }

    #[test]
    fn login_keeps_a_rename_that_lands_during_verify() {
        use std::sync::Arc;
        use std::thread;

        let (_dir, adapter) = adapter_without_auto_create();
        let adapter = Arc::new(adapter);
        let user = adapter
            .create_user(&CreateUserRequest {
                username: "carol".to_string(),
                name: "Carol".to_string(),
                password: "secret".to_string(),
            })
            .unwrap();
        let gates = install_pause(&adapter);
        let logging_in = Arc::clone(&adapter);
        let join = thread::spawn(move || {
            logging_in.login(&LoginCredentials {
                username: "carol".to_string(),
                password: Some("secret".to_string()),
            })
        });
        gates.0.wait();
        adapter
            .update_user(
                &user.id,
                &UpdateUserRequest {
                    name: Some("Renamed".to_string()),
                },
            )
            .unwrap();
        gates.1.wait();

        let session = join.join().expect("login thread").unwrap();
        assert_eq!(session.user.name, "Renamed");
        let identity = adapter.identities.read().unwrap()["carol"].clone();
        assert_eq!(identity.name, "Renamed");
        assert!(identity.last_login_at.is_some());
    }

    #[test]
    fn update_project_member_does_not_resurrect_a_removed_row() {
        use std::sync::Arc;
        use std::thread;

        let (dir, adapter) = adapter_without_auto_create();
        let adapter = Arc::new(adapter);
        adapter
            .add_project_member("alice/shop", "bob", ProjectRole::Editor)
            .unwrap();
        let gates = install_pause(&adapter);
        let updating = Arc::clone(&adapter);
        let join = thread::spawn(move || {
            updating.update_project_member("alice/shop", "bob", ProjectRole::Developer)
        });
        gates.0.wait();
        // The pause is between the read and the persist. The writer is still
        // held, so remove waits and cannot lose to the snapshot. On c184b5a
        // nothing is held there: remove finishes, then the update writes the
        // row back.
        let writer_held = adapter.project_members_writer.try_lock().is_err();
        let removing = Arc::clone(&adapter);
        let removed = thread::spawn(move || removing.remove_project_member("alice/shop", "bob"));
        if writer_held {
            gates.1.wait();
            join.join().expect("update thread").unwrap();
            removed.join().expect("remove thread").unwrap();
        } else {
            removed.join().expect("remove thread").unwrap();
            gates.1.wait();
            let _ = join.join().expect("update thread");
        }
        assert!(
            adapter
                .list_project_members("alice/shop")
                .unwrap()
                .is_empty(),
            "removed row was written back"
        );
        assert!(
            writer_held,
            "update released the project-members writer between the read and the persist"
        );
        assert!(!dir
            .path()
            .join("project_members/alice/shop/bob.json")
            .exists());
    }

    #[test]
    fn concurrent_person_and_org_create_of_one_name_yields_one_account() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let (dir, adapter) = adapter_without_auto_create();
        let adapter = Arc::new(adapter);
        let n_each = 4;
        let barrier = Arc::new(Barrier::new(n_each * 2));
        let mut joins = Vec::new();
        for _ in 0..n_each {
            {
                let adapter = Arc::clone(&adapter);
                let barrier = Arc::clone(&barrier);
                joins.push(thread::spawn(move || {
                    barrier.wait();
                    adapter
                        .create_user(&CreateUserRequest {
                            username: "sam".to_string(),
                            name: "Sam".to_string(),
                            password: "secret".to_string(),
                        })
                        .map(|_| AccountType::Person)
                }));
            }
            {
                let adapter = Arc::clone(&adapter);
                let barrier = Arc::clone(&barrier);
                // On main the taken-name check is a read lock dropped before
                // the argon2 hash. Sleeping only the org threads lets that
                // check pass and the org insert land inside the hash, so both
                // creates return Ok. Here the check and the insert share the
                // accounts writer, taken after the hash.
                joins.push(thread::spawn(move || {
                    barrier.wait();
                    thread::sleep(std::time::Duration::from_millis(5));
                    adapter
                        .create_org("sam", "alice")
                        .map(|account| account.account_type)
                }));
            }
        }

        let mut created = Vec::new();
        for join in joins {
            match join.join().expect("create thread") {
                Ok(kind) => created.push(kind),
                Err(AuthError::UserAlreadyExists) => {}
                Err(err) => panic!("unexpected create error: {err}"),
            }
        }
        assert_eq!(created.len(), 1, "{created:?}");
        assert_eq!(
            adapter
                .accounts
                .read()
                .unwrap()
                .get("sam")
                .unwrap()
                .account_type,
            created[0]
        );

        let on_disk: StoredAccount = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("accounts/sam.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(on_disk.account_type, created[0]);

        drop(adapter);
        let reloaded = LocalAuthAdapter::new(dir.path(), false);
        assert_eq!(
            reloaded
                .accounts
                .read()
                .unwrap()
                .get("sam")
                .unwrap()
                .account_type,
            created[0]
        );
        assert_eq!(
            reloaded.identities.read().unwrap().contains_key("sam"),
            created[0] == AccountType::Person
        );
    }

    #[test]
    fn concurrent_login_auto_create_and_org_create_yields_one_account() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let (dir, adapter) = adapter();
        let adapter = Arc::new(adapter);
        let n_each = 4;
        let barrier = Arc::new(Barrier::new(n_each * 2));
        let mut joins = Vec::new();
        for _ in 0..n_each {
            {
                let adapter = Arc::clone(&adapter);
                let barrier = Arc::clone(&barrier);
                joins.push(thread::spawn(move || {
                    barrier.wait();
                    adapter
                        .login(&LoginCredentials {
                            username: "sam".to_string(),
                            password: Some("secret".to_string()),
                        })
                        .map(|_| "login")
                }));
            }
            {
                let adapter = Arc::clone(&adapter);
                let barrier = Arc::clone(&barrier);
                joins.push(thread::spawn(move || {
                    barrier.wait();
                    thread::sleep(std::time::Duration::from_millis(5));
                    adapter.create_org("sam", "alice").map(|_| "org")
                }));
            }
        }

        let mut org_ok = 0;
        let mut login_ok = 0;
        for join in joins {
            match join.join().expect("create thread") {
                Ok("org") => org_ok += 1,
                Ok("login") => login_ok += 1,
                Ok(other) => panic!("unexpected ok: {other}"),
                Err(AuthError::UserAlreadyExists | AuthError::InvalidCredentials) => {}
                Err(err) => panic!("unexpected create error: {err}"),
            }
        }
        // On main both sides return Ok: the login check has already passed
        // before the org insert. A later login of the winning person is also
        // Ok, so the signal is an org create and a login both succeeding.
        assert!(
            org_ok == 0 || login_ok == 0,
            "org_ok={org_ok} login_ok={login_ok}"
        );
        let kind = adapter
            .accounts
            .read()
            .unwrap()
            .get("sam")
            .unwrap()
            .account_type;
        drop(adapter);
        let reloaded = LocalAuthAdapter::new(dir.path(), true);
        assert_eq!(
            reloaded
                .accounts
                .read()
                .unwrap()
                .get("sam")
                .unwrap()
                .account_type,
            kind
        );
        assert_eq!(
            reloaded.identities.read().unwrap().contains_key("sam"),
            kind == AccountType::Person
        );
    }
}
