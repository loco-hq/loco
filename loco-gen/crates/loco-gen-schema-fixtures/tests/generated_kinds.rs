//! End-to-end checks on generated code: a `kind: files` type and an ordinary
//! document type load from the same instances directory and behave as their
//! kind says they should.

use loco_gen_schema_fixtures::{AssetTree, SchemaStore, Widget, WidgetUpdate};
use loco_schema_runtime::{Error, FileTree};

const KEY: &str = "ben/blog/versions/0.0.1-dev/asset_tree";

fn tree(files: &[(&str, &str)]) -> FileTree {
    let mut t = FileTree::new();
    for (path, body) in files {
        t.insert(path, body.as_bytes().to_vec()).unwrap();
    }
    t
}

#[test]
fn file_tree_type_generates_path_helpers() {
    let path = AssetTree::to_path("ben/blog", "0.0.1-dev");
    assert_eq!(path, KEY);

    let vars = AssetTree::from_path(&path).unwrap();
    assert_eq!(vars.get("project").unwrap(), "ben/blog");
    assert_eq!(vars.get("version").unwrap(), "0.0.1-dev");

    // A key for another type does not match.
    assert!(AssetTree::from_path("ben/blog/widgets/hero").is_none());
}

#[test]
fn write_load_replace_and_delete_a_file_tree() {
    let dir = tempfile::tempdir().unwrap();

    let store = SchemaStore::load(dir.path()).unwrap();
    // A directory with no trees at all is not a boot failure.
    assert!(store.asset_trees().list_all().is_empty());
    assert!(store.asset_trees().read_tree(KEY).unwrap().is_none());
    assert!(store
        .asset_trees()
        .read_file(KEY, "index.html")
        .unwrap()
        .is_none());

    let inst = store
        .asset_trees()
        .put(
            KEY,
            &tree(&[("index.html", "v1"), ("assets/app.js", "one")]),
        )
        .unwrap();
    assert_eq!(inst.project(), "ben/blog");
    assert_eq!(inst.version(), "0.0.1-dev");

    // Stored as a directory at the pathTemplate — no `.yaml` suffix.
    assert!(dir.path().join(KEY).is_dir());
    assert!(!dir.path().join(format!("{KEY}.yaml")).exists());

    // A fresh load finds the tree on disk.
    let reloaded = SchemaStore::load(dir.path()).unwrap();
    assert!(reloaded.asset_trees().has(KEY));
    assert_eq!(
        reloaded.asset_trees().read_file(KEY, "index.html").unwrap(),
        Some(b"v1".to_vec())
    );
    assert_eq!(
        reloaded.asset_trees().list_files(KEY).unwrap(),
        Some(vec!["assets/app.js".to_string(), "index.html".to_string()])
    );

    // Writing replaces the whole tree, atomically.
    reloaded
        .asset_trees()
        .put(KEY, &tree(&[("index.html", "v2")]))
        .unwrap();
    assert_eq!(
        reloaded.asset_trees().list_files(KEY).unwrap(),
        Some(vec!["index.html".to_string()])
    );
    assert_eq!(
        reloaded.asset_trees().read_file(KEY, "index.html").unwrap(),
        Some(b"v2".to_vec())
    );

    reloaded.asset_trees().delete(KEY).unwrap();
    assert!(!reloaded.asset_trees().has(KEY));
    assert!(!dir.path().join(KEY).exists());
    assert!(matches!(
        reloaded.asset_trees().delete(KEY),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn file_trees_and_documents_share_an_instances_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = SchemaStore::load(dir.path()).unwrap();

    store
        .widgets()
        .create(Widget::new(
            "ben/blog".to_string(),
            "hero".to_string(),
            "Hero".to_string(),
        ))
        .unwrap();
    store
        .asset_trees()
        .put(KEY, &tree(&[("index.html", "v1")]))
        .unwrap();

    let reloaded = SchemaStore::load(dir.path()).unwrap();
    assert_eq!(
        reloaded
            .widgets()
            .get("ben/blog/widgets/hero")
            .unwrap()
            .label(),
        "Hero"
    );
    assert!(reloaded.asset_trees().has(KEY));

    // Document behavior is untouched: YAML file, patch updates, delete.
    assert!(dir.path().join("ben/blog/widgets/hero.yaml").is_file());
    let updated = reloaded
        .widgets()
        .update(
            "ben/blog/widgets/hero",
            WidgetUpdate {
                label: Some("Hero 2".to_string()),
            },
        )
        .unwrap();
    assert_eq!(updated.label(), "Hero 2");
}

#[test]
fn prefix_delete_and_copy_reach_file_trees() {
    let dir = tempfile::tempdir().unwrap();
    let store = SchemaStore::load(dir.path()).unwrap();

    store
        .asset_trees()
        .put(KEY, &tree(&[("index.html", "draft")]))
        .unwrap();
    store
        .asset_trees()
        .put(
            "ben/shop/versions/0.0.1-dev/asset_tree",
            &tree(&[("index.html", "shop")]),
        )
        .unwrap();

    // The copy half of a future copy-version: same suffix, new version prefix.
    let copied = store
        .asset_trees()
        .copy_by_prefix("ben/blog/versions/0.0.1-dev/", "ben/blog/versions/0.0.1/")
        .unwrap();
    assert_eq!(
        copied,
        vec!["ben/blog/versions/0.0.1/asset_tree".to_string()]
    );
    assert_eq!(
        store
            .asset_trees()
            .read_file("ben/blog/versions/0.0.1/asset_tree", "index.html")
            .unwrap(),
        Some(b"draft".to_vec())
    );

    // The delete half: a project delete cascades into file trees.
    let deleted = store.asset_trees().delete_by_prefix("ben/blog/").unwrap();
    assert_eq!(deleted.len(), 2);
    assert!(!dir.path().join("ben/blog/versions").exists());
    assert!(store
        .asset_trees()
        .has("ben/shop/versions/0.0.1-dev/asset_tree"));
}
