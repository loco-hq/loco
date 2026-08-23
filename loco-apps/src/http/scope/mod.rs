//! Request-scoped accessors that pre-resolve "what's visible from here".
//!
//! - [`ProjectScope`]: pinned to a project; configs only (project, dataset, site).
//! - [`SiteScope`]: pinned to a site (X-Project-Id + X-Site-Id headers).
//!   The single home for request-time authz (`require_authenticated`,
//!   `require_developer`, `require_can_write_data`) and the read-only
//!   schema view used by data routes. Access is membership, not the site.
//! - [`VersionScope`]: writable `VersionSchema` for the path
//!   `{user}/{project}/{version}`. Developer (or org owner) required.
//!   Used by `/schema` writes. Site headers are not required.
//! - [`VersionReadScope`]: read-only view for GET `/schema`. Developers
//!   and editors can read any version of a project they belong to.
//!   `public` (and authenticated non-members) can read a site's pinned
//!   version when that site assigns at least one permission set to
//!   `public` (`X-Project-Id` + `X-Site-Id` required).
//! - [`ConfigProjectScope`] / [`ConfigUserScope`]: `/config` routes.
//!   The first targets an existing `{user}/{project}` (developer required).
//!   The second is authenticated-only for routes with no project in the
//!   URL (project create, org create, project list).
//! - [`CollectionScope`] / [`RecordScope`] layer on top of `SiteScope` for
//!   data routes that need a specific collection or record. Public CRUD
//!   follows the verbs on permission sets the site assigns to `public`.

mod collection;
mod config;
mod helpers;
mod project;
mod record;
mod site;
mod version;

pub use collection::CollectionScope;
pub use config::{ConfigProjectScope, ConfigUserScope};
pub use project::ProjectScope;
pub use record::RecordScope;
pub use site::SiteScope;
pub use version::{VersionReadScope, VersionScope};
