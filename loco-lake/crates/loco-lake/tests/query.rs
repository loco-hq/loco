//! `DataAdapter::query` semantics, run identically against every adapter so
//! the memory evaluator and the sqlite compiler cannot drift apart.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use loco_lake::{
    CompareOp, DataAdapter, Direction, Error, FieldRef, Filter, InMemoryAdapter, InsertRequest,
    LakeQuery, OrderKey, Page, SqliteAdapter, SystemField, Value,
};

const DS: &str = "ben/test/dev";
const COLL: &str = "ben/test.item";

fn adapters() -> Vec<(&'static str, Box<dyn DataAdapter>)> {
    vec![
        ("memory", Box::new(InMemoryAdapter::new())),
        (
            "sqlite",
            Box::new(SqliteAdapter::new(Path::new(":memory:")).unwrap()),
        ),
    ]
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn i(v: i64) -> Value {
    Value::Integer(v)
}

fn f(v: f64) -> Value {
    Value::Float(v)
}

/// Insert one record tagged `tag` into `coll`; returns its id.
fn put(a: &dyn DataAdapter, coll: &str, tag: &str, fields: Vec<(&str, Value)>) -> String {
    let mut map: HashMap<String, Value> = fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    map.insert("tag".to_string(), s(tag));
    a.insert(
        DS,
        coll,
        InsertRequest {
            user: "alice".to_string(),
            fields: map,
        },
    )
    .unwrap()
    .id
}

/// Tag → id for the shared fixture. Values are chosen to hit kind edges:
/// integer vs float, a number drifted to a string, explicit null vs
/// missing, and booleans (which sqlite's json_extract reads as 0 / 1).
fn seed(a: &dyn DataAdapter) -> HashMap<String, String> {
    let rows: Vec<(&str, Vec<(&str, Value)>)> = vec![
        (
            "a",
            vec![
                ("qty", i(3)),
                ("name", s("apple")),
                ("flag", Value::Boolean(true)),
            ],
        ),
        ("b", vec![("qty", f(3.0)), ("name", s("Banana"))]),
        ("c", vec![("qty", s("3")), ("name", s("cherry"))]),
        ("d", vec![("qty", Value::Null), ("name", s("date"))]),
        ("e", vec![("name", s("elder"))]),
        (
            "f",
            vec![
                ("qty", i(1)),
                ("name", s("fig")),
                ("flag", Value::Boolean(false)),
            ],
        ),
        (
            "g",
            vec![("qty", f(10.5)), ("name", s("grape")), ("flag", i(1))],
        ),
        (
            "h",
            vec![("qty", Value::Boolean(true)), ("name", s("hazel"))],
        ),
    ];
    rows.into_iter()
        .map(|(tag, fields)| (tag.to_string(), put(a, COLL, tag, fields)))
        .collect()
}

fn run(a: &dyn DataAdapter, q: LakeQuery) -> Page {
    a.query(DS, &[q]).unwrap().pop().unwrap()
}

fn tags(page: &Page) -> Vec<String> {
    page.records
        .iter()
        .map(|r| match r.fields.get("tag") {
            Some(Value::String(t)) => t.clone(),
            other => panic!("record without tag: {other:?}"),
        })
        .collect()
}

fn cmp(field: &str, op: CompareOp, value: Value) -> Filter {
    Filter::Compare {
        field: FieldRef::field(field),
        op,
        value,
    }
}

fn filtered(filter: Filter) -> LakeQuery {
    LakeQuery {
        filter: Some(filter),
        ..LakeQuery::new(COLL, 100)
    }
}

fn set(tags: &str) -> BTreeSet<String> {
    tags.chars().map(|c| c.to_string()).collect()
}

#[test]
fn filters() {
    use CompareOp::*;
    let cases: Vec<(&str, Filter, &str)> = vec![
        ("eq int matches int and float", cmp("qty", Eq, i(3)), "ab"),
        (
            "eq float matches int and float",
            cmp("qty", Eq, f(3.0)),
            "ab",
        ),
        (
            "eq string does not match numbers",
            cmp("qty", Eq, s("3")),
            "c",
        ),
        (
            "eq null matches null and missing",
            cmp("qty", Eq, Value::Null),
            "de",
        ),
        ("ne null", cmp("qty", Ne, Value::Null), "abcfgh"),
        (
            "ne is not eq, so null and drift match",
            cmp("qty", Ne, i(3)),
            "cdefgh",
        ),
        ("gt number", cmp("qty", Gt, i(2)), "abg"),
        ("gte number", cmp("qty", Gte, i(3)), "abg"),
        (
            "lt number skips null, bool, string",
            cmp("qty", Lt, i(3)),
            "f",
        ),
        ("lte number", cmp("qty", Lte, f(3.0)), "abf"),
        (
            "lt string only matches strings",
            cmp("qty", Lt, s("4")),
            "c",
        ),
        ("strings compare as bytes", cmp("name", Lt, s("b")), "ab"),
        (
            "eq true is not integer 1",
            cmp("flag", Eq, Value::Boolean(true)),
            "a",
        ),
        ("eq 1 is not boolean true", cmp("flag", Eq, i(1)), "g"),
        ("eq false", cmp("flag", Eq, Value::Boolean(false)), "f"),
        ("eq 0 matches nothing", cmp("flag", Eq, i(0)), ""),
        (
            "exists true",
            Filter::Exists {
                field: FieldRef::field("qty"),
                exists: true,
            },
            "abcfgh",
        ),
        (
            "exists false",
            Filter::Exists {
                field: FieldRef::field("qty"),
                exists: false,
            },
            "de",
        ),
        (
            "in with null",
            Filter::In {
                field: FieldRef::field("qty"),
                values: vec![i(3), s("3"), Value::Null],
            },
            "abcde",
        ),
        (
            "not gt includes null, missing, drift",
            Filter::Not(Box::new(cmp("qty", Gt, i(2)))),
            "cdefh",
        ),
        (
            "and",
            Filter::And(vec![cmp("qty", Gte, i(1)), cmp("name", Gt, s("b"))]),
            "fg",
        ),
        (
            "or",
            Filter::Or(vec![cmp("qty", Eq, i(1)), cmp("qty", Eq, Value::Null)]),
            "def",
        ),
        ("empty and is true", Filter::And(vec![]), "abcdefgh"),
        ("empty or is false", Filter::Or(vec![]), ""),
        (
            "not of empty or",
            Filter::Not(Box::new(Filter::Or(vec![]))),
            "abcdefgh",
        ),
        (
            "system field",
            Filter::Compare {
                field: FieldRef::System(SystemField::CreatedBy),
                op: Eq,
                value: s("alice"),
            },
            "abcdefgh",
        ),
        (
            "system field ne",
            Filter::Compare {
                field: FieldRef::System(SystemField::Owner),
                op: Ne,
                value: s("alice"),
            },
            "",
        ),
    ];
    for (name, a) in adapters() {
        seed(a.as_ref());
        for (case, filter, want) in &cases {
            let got: BTreeSet<String> = tags(&run(a.as_ref(), filtered(filter.clone())))
                .into_iter()
                .collect();
            assert_eq!(got, set(want), "{name}: {case}");
        }
    }
}

#[test]
fn filter_by_id() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(
            a.as_ref(),
            filtered(Filter::Compare {
                field: FieldRef::System(SystemField::Id),
                op: CompareOp::Eq,
                value: s(&ids["c"]),
            }),
        );
        assert_eq!(tags(&page), vec!["c"], "{name}");
    }
}

#[test]
fn large_integers_compare_exactly_with_floats() {
    // 2^53 + 1 is not representable as f64; it rounds to 2^53.
    let big = 9_007_199_254_740_993_i64;
    for (name, a) in adapters() {
        put(a.as_ref(), COLL, "x", vec![("n", i(big))]);
        let eq = run(a.as_ref(), filtered(cmp("n", CompareOp::Eq, f(big as f64))));
        assert_eq!(tags(&eq), Vec::<String>::new(), "{name}: eq");
        let gt = run(a.as_ref(), filtered(cmp("n", CompareOp::Gt, f(big as f64))));
        assert_eq!(tags(&gt), vec!["x"], "{name}: gt");
    }
}

#[test]
fn awkward_field_names() {
    for (name, a) in adapters() {
        put(
            a.as_ref(),
            COLL,
            "x",
            vec![("a.b", i(1)), ("c[0]", i(2)), ("é", i(3))],
        );
        put(a.as_ref(), COLL, "y", vec![("a", i(1))]);
        for (field, value) in [("a.b", 1), ("c[0]", 2), ("é", 3)] {
            let page = run(a.as_ref(), filtered(cmp(field, CompareOp::Eq, i(value))));
            assert_eq!(tags(&page), vec!["x"], "{name}: {field}");
        }
    }
}

/// Expected tag order when each group ties on the sort key: ties fall to
/// `id` ascending.
fn by_groups(groups: &[&str], ids: &HashMap<String, String>) -> Vec<String> {
    groups
        .iter()
        .flat_map(|g| {
            let mut tags: Vec<String> = g.chars().map(|c| c.to_string()).collect();
            tags.sort_by_key(|t| ids[t].clone());
            tags
        })
        .collect()
}

fn ordered(key: OrderKey, limit: usize) -> LakeQuery {
    LakeQuery {
        order: vec![key],
        ..LakeQuery::new(COLL, limit)
    }
}

#[test]
fn order_asc_puts_null_first_then_kinds() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(
            a.as_ref(),
            ordered(OrderKey::asc(FieldRef::field("qty")), 100),
        );
        let want = by_groups(&["de", "h", "f", "ab", "g", "c"], &ids);
        assert_eq!(tags(&page), want, "{name}");
        assert_eq!(page.next, None, "{name}");
    }
}

#[test]
fn order_desc_puts_null_last() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(
            a.as_ref(),
            ordered(OrderKey::desc(FieldRef::field("qty")), 100),
        );
        let want = by_groups(&["c", "g", "ab", "f", "h", "de"], &ids);
        assert_eq!(tags(&page), want, "{name}");
    }
}

#[test]
fn strings_order_as_bytes() {
    for (name, a) in adapters() {
        for (tag, v) in [("z", "z"), ("e", "é"), ("u", "Z"), ("l", "a")] {
            put(a.as_ref(), COLL, tag, vec![("name", s(v))]);
        }
        let page = run(
            a.as_ref(),
            ordered(OrderKey::asc(FieldRef::field("name")), 100),
        );
        assert_eq!(tags(&page), vec!["u", "l", "z", "e"], "{name}");
    }
}

#[test]
fn default_order_is_id() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(a.as_ref(), LakeQuery::new(COLL, 100));
        assert_eq!(tags(&page), by_groups(&["abcdefgh"], &ids), "{name}");
    }
}

#[test]
fn next_is_the_last_records_order_key() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(
            a.as_ref(),
            ordered(OrderKey::asc(FieldRef::field("qty")), 3),
        );
        let want = by_groups(&["de", "h", "f", "ab", "g", "c"], &ids);
        assert_eq!(tags(&page), want[..3], "{name}");
        assert_eq!(
            page.next,
            Some(vec![Value::Boolean(true), s(&ids["h"])]),
            "{name}"
        );
    }
}

/// Walk every page of `q` by feeding `next` back as `after`.
fn walk(a: &dyn DataAdapter, q: LakeQuery) -> Vec<String> {
    let mut out = Vec::new();
    let mut q = q;
    for _ in 0..100 {
        let page = run(a, q.clone());
        assert!(page.records.len() <= q.limit);
        out.extend(tags(&page));
        match page.next {
            Some(next) => q.after = Some(next),
            None => return out,
        }
    }
    panic!("pagination did not terminate");
}

#[test]
fn keyset_pages_cover_every_record_once() {
    for (name, a) in adapters() {
        seed(a.as_ref());
        for key in [
            OrderKey::asc(FieldRef::field("qty")),
            OrderKey::desc(FieldRef::field("qty")),
            OrderKey::asc(FieldRef::field("flag")),
            OrderKey::desc(FieldRef::System(SystemField::Id)),
        ] {
            let all = tags(&run(a.as_ref(), ordered(key.clone(), 100)));
            for limit in 1..=9 {
                let paged = walk(a.as_ref(), ordered(key.clone(), limit));
                assert_eq!(paged, all, "{name}: {key:?} limit {limit}");
            }
        }
    }
}

#[test]
fn keyset_over_floats() {
    for (name, a) in adapters() {
        for (tag, v) in [
            ("p", 0.1),
            ("q", 0.2),
            ("r", 0.1 + 0.2),
            ("s", 0.3),
            ("t", -1e-7),
        ] {
            put(a.as_ref(), COLL, tag, vec![("x", f(v))]);
        }
        let q = ordered(OrderKey::asc(FieldRef::field("x")), 1);
        assert_eq!(walk(a.as_ref(), q), vec!["t", "p", "q", "s", "r"], "{name}");
    }
}

#[test]
fn keyset_is_stable_under_inserts_between_pages() {
    for (name, a) in adapters() {
        for (tag, n) in [("a", 10), ("b", 20), ("c", 30), ("d", 40)] {
            put(a.as_ref(), COLL, tag, vec![("n", i(n))]);
        }
        let q = ordered(OrderKey::asc(FieldRef::field("n")), 2);
        let first = run(a.as_ref(), q.clone());
        assert_eq!(tags(&first), vec!["a", "b"], "{name}");

        // Before the cursor: never seen. After it: picked up.
        put(a.as_ref(), COLL, "x", vec![("n", i(5))]);
        put(a.as_ref(), COLL, "y", vec![("n", i(25))]);
        let rest = walk(
            a.as_ref(),
            LakeQuery {
                after: first.next,
                ..q
            },
        );
        assert_eq!(rest, vec!["y", "c", "d"], "{name}");
    }
}

#[test]
fn filter_and_order_and_after_combine() {
    for (name, a) in adapters() {
        seed(a.as_ref());
        let q = LakeQuery {
            filter: Some(cmp("qty", CompareOp::Ne, Value::Null)),
            order: vec![OrderKey {
                field: FieldRef::field("name"),
                dir: Direction::Desc,
            }],
            ..LakeQuery::new(COLL, 2)
        };
        assert_eq!(
            walk(a.as_ref(), q),
            vec!["h", "g", "f", "c", "a", "b"],
            "{name}"
        );
    }
}

#[test]
fn projection_keeps_named_fields_and_all_system_fields() {
    for (name, a) in adapters() {
        let id = put(a.as_ref(), COLL, "x", vec![("n", i(1)), ("m", i(2))]);
        let page = run(
            a.as_ref(),
            LakeQuery {
                fields: Some(vec!["n".to_string(), "absent".to_string()]),
                ..LakeQuery::new(COLL, 10)
            },
        );
        let r = &page.records[0];
        assert_eq!(r.id, id, "{name}");
        assert_eq!(r.created_by, "alice", "{name}");
        assert_eq!(r.dataset_id, DS, "{name}");
        assert_eq!(r.fields, HashMap::from([("n".to_string(), i(1))]), "{name}");
    }
}

#[test]
fn projection_does_not_affect_order_or_next() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(
            a.as_ref(),
            LakeQuery {
                fields: Some(vec![]),
                ..ordered(OrderKey::asc(FieldRef::field("name")), 1)
            },
        );
        assert!(page.records[0].fields.is_empty(), "{name}");
        // "Banana" < "apple" as bytes.
        assert_eq!(page.next, Some(vec![s("Banana"), s(&ids["b"])]), "{name}");
    }
}

#[test]
fn batch_returns_pages_in_order_scoped_to_dataset() {
    for (name, a) in adapters() {
        put(a.as_ref(), COLL, "x", vec![]);
        put(a.as_ref(), "ben/test.other", "y", vec![]);
        a.insert(
            "ben/test/prod",
            COLL,
            InsertRequest {
                user: "alice".to_string(),
                fields: HashMap::from([("tag".to_string(), s("z"))]),
            },
        )
        .unwrap();
        let pages = a
            .query(
                DS,
                &[
                    LakeQuery::new("ben/test.other", 10),
                    LakeQuery::new(COLL, 10),
                    LakeQuery::new("ben/test.missing", 10),
                ],
            )
            .unwrap();
        let got: Vec<Vec<String>> = pages.iter().map(tags).collect();
        assert_eq!(got, vec![vec!["y"], vec!["x"], vec![]], "{name}");
        assert!(pages.iter().all(|p| p.next.is_none()), "{name}");
        assert!(a.query(DS, &[]).unwrap().is_empty(), "{name}");
    }
}

#[test]
fn invalid_queries_are_rejected() {
    let bad: Vec<(&str, LakeQuery)> = vec![
        ("limit 0", LakeQuery::new(COLL, 0)),
        (
            "empty in",
            filtered(Filter::In {
                field: FieldRef::field("qty"),
                values: vec![],
            }),
        ),
        (
            "ordering against null",
            filtered(cmp("qty", CompareOp::Gt, Value::Null)),
        ),
        (
            "ordering against boolean",
            filtered(cmp("qty", CompareOp::Lt, Value::Boolean(true))),
        ),
        (
            "quote in a field name",
            filtered(cmp("a\"b", CompareOp::Eq, i(1))),
        ),
        (
            "quote in an order key",
            ordered(OrderKey::asc(FieldRef::field("a\"b")), 1),
        ),
        (
            "after shorter than the effective order",
            LakeQuery {
                after: Some(vec![i(1)]),
                ..ordered(OrderKey::asc(FieldRef::field("qty")), 1)
            },
        ),
    ];
    for (name, a) in adapters() {
        for (case, q) in &bad {
            // A bad query fails the call even beside a good one.
            let res = a.query(DS, &[LakeQuery::new(COLL, 1), q.clone()]);
            assert!(
                matches!(res, Err(Error::InvalidQuery(_))),
                "{name}: {case}: {res:?}"
            );
        }
    }
}

#[test]
fn explicit_trailing_id_is_not_doubled() {
    for (name, a) in adapters() {
        let ids = seed(a.as_ref());
        let page = run(
            a.as_ref(),
            ordered(OrderKey::desc(FieldRef::System(SystemField::Id)), 1),
        );
        let last = ids.values().max().unwrap();
        assert_eq!(page.next, Some(vec![s(last)]), "{name}");
    }
}
