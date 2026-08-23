//! Request-scoped accessors that pre-resolve "what's visible from here".
//!
//! - [`ProjectScope`]: pinned to a project; configs only (project, dataset, site).
//! - [`SiteScope`]: pinned to a site (X-Project-Id + X-Site-Id headers).
//!   The single home for request-time authz (`require_authenticated`,
//!   `require_developer`, `require_can_write_data`) and the read-only
//!   schema view used by data routes. Access is membership, not the site.
//! - [`VersionScope`]: a `SiteScope` plus a writable `VersionSchema` for
//!   the `{user}/{project}/{version}` triple in the path. Has no authz
//!   logic of its own — composes `SiteScope`'s checks.
//! - [`ConfigProjectScope`] / [`ConfigUserScope`]: `/config` routes.
//!   The first targets an existing `{user}/{project}` (developer required).
//!   The second is authenticated-only for routes with no project in the
//!   URL (project create, org create, project list).
//! - [`CollectionScope`] / [`RecordScope`] layer on top of `SiteScope` for
//!   data routes that need a specific collection or record. Public list/get
//!   and insert are the union of permission sets the site assigns to `public`.

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
pub use version::VersionScope;
