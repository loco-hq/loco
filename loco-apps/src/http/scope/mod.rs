//! Request-scoped accessors that pre-resolve "what's visible from here".
//!
//! - [`ProjectScope`]: pinned to a project; configs only (project, dataset, site).
//! - [`VersionScope`]: pinned to a (project, version); versioned metadata
//!   (collection, field, manifest).
//! - [`SiteScope`]: pinned to a site, which itself pins a version + dataset;
//!   the natural shape for data API routes and pre-auth flows.
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
