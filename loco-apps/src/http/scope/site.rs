use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::{session_or_public, AuthSession, AuthUser, ProjectRole, PUBLIC_USERNAME};
use crate::http::authz::{forbidden, public_may, DataVerb};
use crate::http::response::error_response;
use crate::http::version_schema::{AddressResolution, CollectionAddress, VersionSchema};
use crate::server::AppState;
use crate::{PermissionSet, Site};

use super::helpers::{read_project_id, read_site_id};
use super::project::ProjectScope;

pub struct SiteScope {
    pub project: ProjectScope,
    pub site: Arc<Site>,
    /// Always populated — synthesized as `AuthSession::public()` when no
    /// `Authorization` header is sent, so every scope has a principal. A
    /// header that fails to authenticate is a 401, not `public`.
    pub auth: AuthSession,
    /// Read-only scoped schema view — bounded by this site's project+version
    /// plus its installed dependencies. Use this instead of `state.schema`
    /// from handlers under `/data` so they can't reach metadata for
    /// unrelated projects or non-installed versions, and can't mutate
    /// metadata at all (writes go through `VersionScope`).
    pub schema: VersionSchema,
}

impl SiteScope {
    /// `{account}/{project}/{site_name}` — the site named by request headers,
    /// not the logged-in identity.
    pub fn qualified_site_id(&self) -> String {
        format!("{}/{}", self.project.project_id(), self.site.name())
    }

    pub fn user(&self) -> &AuthUser {
        &self.auth.user
    }

    pub fn dataset_id(&self) -> String {
        let ds = self.site.dataset();
        let ds = if ds.is_empty() { self.site.name() } else { ds };
        format!("{}/{}", self.project.project_id(), ds)
    }

    /// The collection address `name` refers to in this site's pinned version.
    ///
    /// Resolving through the site's `VersionSchema` rather than the global
    /// `SchemaStore` is what keeps existence agreeing with validation. A prefix
    /// scan of `{user}/{project}/` matches *every* version of the project, so a
    /// site pinned to a published version could reach a collection that only
    /// exists in a draft — and a permission set naming it made that an
    /// unauthenticated read. The scoped view sees only what the site pins.
    ///
    /// `name` follows the rule in CLAUDE.md ("Name resolution"): bare is this
    /// project's collection, `{account}/{project}.{local}` a direct
    /// dependency's. `local` may be `{integration}:{name}`. A name with no
    /// `:` is an ordinary collection. A transitive or unknown project is a
    /// 404 like any other missing collection, before auth. An address that
    /// names both a standard collection and a custom collection still
    /// resolves: the handler answers 409 after its access check.
    pub fn require_collection(&self, name: &str) -> Result<CollectionAddress, Response> {
        match self.schema.collection_address(name) {
            AddressResolution::Resolved(address) => Ok(address),
            AddressResolution::Missing => Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown collection: {name}"),
            )),
        }
    }

    // --- Authz checks ---
    //
    // Token → identity → union of org role + project role. Capability is
    // on the member, not the site named by the request headers.

    /// Reject synthesized public sessions. Use on routes that require a
    /// real logged-in user (writes, anything mutating state).
    pub fn require_authenticated(&self) -> Result<(), Response> {
        if self.auth.user.username == PUBLIC_USERNAME {
            Err(error_response(
                StatusCode::UNAUTHORIZED,
                "authentication required",
            ))
        } else {
            Ok(())
        }
    }

    pub fn project_role(&self, project_id: &str) -> Result<Option<ProjectRole>, Response> {
        self.project
            .state
            .auth_adapter
            .project_access(&self.auth.user.username, project_id)
            .map_err(crate::auth::auth_error_to_response)
    }

    /// `/schema` + `/config` writes: developer, or org owner of the account.
    pub fn require_developer(&self, project_id: &str) -> Result<(), Response> {
        match self.project_role(project_id)? {
            Some(role) if role.can_develop() => Ok(()),
            Some(_) | None => Err(forbidden()),
        }
    }

    pub fn is_public(&self) -> bool {
        self.auth.user.username == PUBLIC_USERNAME
    }

    /// Identity is a project editor/developer (or org owner). `public` never is.
    pub fn has_data_access(&self) -> Result<bool, Response> {
        if self.is_public() {
            return Ok(false);
        }
        match self.project_role(&self.project.project_id())? {
            Some(role) if role.can_edit_data() => Ok(true),
            Some(_) | None => Ok(false),
        }
    }

    /// Permission sets the pinned version's manifest assigns to `public`,
    /// resolved against that version: a bare name is this project's own set,
    /// a dependency's must be qualified. Unknown names are skipped.
    ///
    /// Policy is a property of the snapshot, not of the URL: two sites that
    /// pin the same version cannot disagree about what `public` may do.
    fn public_sets(&self) -> Vec<Arc<PermissionSet>> {
        self.schema
            .public_permission_sets()
            .iter()
            .filter_map(|name| self.schema.permission_set(name))
            .collect()
    }

    /// A public permission set grants `verb` on the collection `name` owned
    /// by `project`.
    pub fn public_allowed(&self, name: &str, project: &str, verb: DataVerb) -> bool {
        public_may(
            self.public_sets().iter().map(|s| s.as_ref()),
            name,
            project,
            verb,
        )
    }

    /// Read on the collection `name` owned by `project`: members with data
    /// access, or anyone when a public set grants `read`. `Err` only when the
    /// membership lookup itself fails.
    pub fn may_read_collection(&self, name: &str, project: &str) -> Result<bool, Response> {
        Ok(self.has_data_access()? || self.public_allowed(name, project, DataVerb::Read))
    }

    /// `GET /actions`: data access, or a read on any collection this version
    /// shows. An action is not a collection, so the fields read rule is
    /// lifted off one collection. A version with nothing readable is members
    /// only.
    pub fn may_read_actions(&self) -> Result<bool, Response> {
        if self.has_data_access()? {
            return Ok(true);
        }
        for address in self.schema.collection_addresses() {
            if self.may_read_collection(&address.local, &address.project)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Member data mutation (developer or editor). Public verbs live on
    /// [`super::CollectionScope`] and follow the site's permission sets.
    /// `POST /actions` uses this and not the public create grant: token-less
    /// `public` cannot run an action.
    pub fn require_can_write_data(&self) -> Result<(), Response> {
        if self.has_data_access()? {
            Ok(())
        } else {
            Err(forbidden())
        }
    }
}

impl FromRequestParts<Arc<AppState>> for SiteScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let (user, project) = read_project_id(parts)?;
        let site_name = read_site_id(parts)?;

        let project_id = format!("{user}/{project}");
        let site = state
            .schema
            .sites()
            .get(&Site::to_path(&project_id, &site_name))
            .ok_or_else(|| {
                error_response(
                    StatusCode::NOT_FOUND,
                    &format!("unknown site: {site_name} in project {project_id}"),
                )
            })?;

        let auth = session_or_public(parts, state).await?;

        let version = site.version().to_string();
        let schema = VersionSchema::new_read_only(state.schema.clone(), &project_id, &version);

        Ok(SiteScope {
            project: ProjectScope {
                user,
                project,
                state: state.clone(),
            },
            site,
            auth,
            schema,
        })
    }
}
