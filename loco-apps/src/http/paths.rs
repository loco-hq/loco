pub fn collection_key(user: &str, project: &str, name: &str) -> String {
    format!("{user}/{project}.{name}")
}
