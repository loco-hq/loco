//! Request-scoped accessors that pre-resolve "what's visible from here".
//!
//! - [`ProjectScope`]: pinned to a project; configs only (project, dataset, site).
//! - [`SiteScope`]: pinned to a site (X-Project-Id + X-Site-Id headers).
//!   The single home for request-time authz (`require_authenticated`,
//!   `require_metadata_editing_site`, `require_can_edit_user`) and the
//!   read-only schema view used by data routes.
//! - [`VersionScope`]: a `SiteScope` plus a writable `VersionSchema` for
//!   the `{user}/{project}/{version}` triple in the path. Has no authz
//!   logic of its own — composes `SiteScope`'s checks.
//! - [`CollectionScope`] / [`RecordScope`] layer on top of `SiteScope` for
//!   data routes that need a specific collection or record.

mod collection;
mod helpers;
mod project;
mod record;
mod site;
mod version;

pub use collection::CollectionScope;
pub use project::ProjectScope;
pub use record::RecordScope;
pub use site::SiteScope;
pub use version::VersionScope;
