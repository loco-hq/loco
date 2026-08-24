/// Lake `collection` column for a record.
///
/// Qualified by the project that **owns** the collection — resolved through the
/// site's `VersionSchema` — not by the project in the request headers. A site
/// can read a collection owned by one of its direct dependencies, and two
/// collections visible from one site can share a bare name, so the owner is
/// what makes the key unambiguous.
///
/// The dataset (`{account}/{project}/{dataset}`) already isolates storage
/// between sites; this disambiguates *within* one dataset.
///
/// Deliberately excludes the version. Records outlive the schema version they
/// were written under, so a version bump must not orphan them.
pub fn collection_key(owner_project: &str, name: &str) -> String {
    format!("{owner_project}.{name}")
}
