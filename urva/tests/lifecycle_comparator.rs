use mongodb::bson::{Document, doc};
use urva::lifecycle::{IndexDiff, NameDrift, compute_diff};
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders")]
#[index(tenant_recent, keys(tenant_id, created_at = -1), unique)]
#[index(open_status, keys(status), partial(status = "open"))]
#[index(order_search,  keys(title = text, body = text), weights(title = 10))]
#[index(expiry, keys(expires_at), ttl = 3600)]
#[index(by_city, keys(city), collation(locale = "en", strength = 2))]
#[index(everything, keys(wildcard), wildcard_projection = "{\"title\": 1}")]
#[index(geo,           keys(location = 2d), bits = 26)]
#[index(ghost, keys(body), hidden)]
#[index(sparse_title, keys(title), sparse)]
#[external_index(legacy_geo)]
pub struct Order {
    pub tenant_id: ObjectId,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub title: String,
    pub body: String,
    pub expires_at: DateTime,
    pub city: String,
    pub location: Vec<f64>,
}

fn server_fixtures() -> Vec<Document> {
    vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "tenant_id": 1, "createdAt": -1 },
            "name": "tenant_recent",
            "unique": true,
        },
        doc! {
            "v": 2,
            "key": { "status": 1 },
            "name": "open_status",

            "partialFilterExpression": { "status": { "$eq": "open" } },
        },
        doc! {
            "v": 2,

            "key": { "_fts": "text", "_ftsx": 1 },
            "name": "order_search",
            "weights": { "body": 1, "title": 10 },
            "default_language": "english",
            "language_override": "language",
            "textIndexVersion": 3,
        },
        doc! {
            "v": 2,
            "key": { "expires_at": 1 },
            "name": "expiry",
            "expireAfterSeconds": 3600,
        },
        doc! {
            "v": 2,
            "key": { "city": 1 },
            "name": "by_city",

            "collation": {
                "locale": "en",
                "caseLevel": false,
                "caseFirst": "off",
                "strength": 2,
                "numericOrdering": false,
                "alternate": "non-ignorable",
                "maxVariable": "punct",
                "normalization": false,
                "backwards": false,
                "version": "57.1",
            },
        },
        doc! {
            "v": 2,
            "key": { "$**": 1 },
            "name": "everything",
            "wildcardProjection": { "title": 1 },
        },
        doc! {
            "v": 2,
            "key": { "location": "2d" },
            "name": "geo",
            "bits": 26,
        },
        doc! {
            "v": 2,
            "key": { "body": 1 },
            "name": "ghost",
            "hidden": true,
        },
        doc! {
            "v": 2,
            "key": { "title": 1 },
            "name": "sparse_title",
            "sparse": true,
        },
        doc! {
            "v": 2,
            "key": { "location": "2dsphere" },
            "name": "legacy_geo",
            "2dsphereIndexVersion": 3,
        },
    ]
}

#[test]
fn expanded_server_output_is_a_no_op() {
    let diff = compute_diff::<Order>(&server_fixtures());
    assert_eq!(
        diff,
        IndexDiff::default(),
        "no-op diff on faithful server state"
    );
    assert!(diff.is_empty());
    assert!(!diff.fails_verify());
}

#[test]
fn missing_declared_index_is_to_create() {
    let mut fixtures = server_fixtures();
    fixtures.retain(|m| m.get_str("name").ok() != Some("tenant_recent"));
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.to_create, ["tenant_recent"]);
    assert!(diff.fails_verify());
}

#[test]
fn undeclared_server_index_is_to_drop_but_does_not_fail_verify() {
    let mut fixtures = server_fixtures();
    fixtures.push(doc! {
        "v": 2, "key": { "somebody_else": 1 }, "name": "somebody_else_idx",
    });
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.to_drop, ["somebody_else_idx"]);
    assert!(!diff.fails_verify(), "to_drop alone does not fail verify");
}

#[test]
fn same_name_different_structure_is_mismatched() {
    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("tenant_recent") {
            *m = doc! {
                "v": 2,
                "key": { "tenant_id": 1, "createdAt": -1 },
                "name": "tenant_recent",

            };
        }
    }
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.mismatched, ["tenant_recent"]);
    assert!(diff.to_create.is_empty(), "mismatch is not creation");
}

#[test]
fn same_structure_other_name_is_name_drift() {
    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("tenant_recent") {
            *m = doc! {
                "v": 2,
                "key": { "tenant_id": 1, "createdAt": -1 },
                "name": "tenant_recent_v1",
                "unique": true,
            };
        }
    }
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(
        diff.name_drift,
        [NameDrift {
            expected: "tenant_recent".into(),
            actual: "tenant_recent_v1".into(),
        }]
    );
    assert!(
        diff.to_drop.is_empty(),
        "a drifted index is never silently dropped"
    );
    assert!(
        diff.fails_verify(),
        "hints resolve by name, so drift fails verify"
    );
}

#[test]
fn drifted_name_with_structural_twin_is_mismatched_not_dropped() {
    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("tenant_recent") {
            *m = doc! {
                "v": 2,
                "key": { "tenant_id": 1, "createdAt": -1, "extra": 1 },
                "name": "tenant_recent",

            };
        }
    }
    fixtures.push(doc! {
        "v": 2,
        "key": { "tenant_id": 1, "createdAt": -1 },
        "name": "old_tenant",
        "unique": true,
    });
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.mismatched, ["tenant_recent"]);
    assert_eq!(diff.to_drop, ["old_tenant"]);
    assert!(
        diff.name_drift.is_empty(),
        "name identity wins over the twin"
    );
    assert!(diff.to_create.is_empty());
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "pairs")]
#[index(a, keys(x))]
#[index(b, keys(y))]
pub struct Pair {
    pub x: i64,
    pub y: i64,
}

#[test]
fn stale_index_under_another_declarations_name_is_mismatched() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! { "v": 2, "key": { "x": 1 }, "name": "b" },
    ];
    let diff = compute_diff::<Pair>(&fixtures);
    assert_eq!(
        diff.mismatched,
        ["b"],
        "name identity binds before the twin scan"
    );
    assert_eq!(diff.to_create, ["a"]);
    assert!(diff.name_drift.is_empty());
    assert!(diff.to_drop.is_empty());
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "ids")]
#[index(by_key, keys(key))]
pub struct IdTwin {
    pub key: String,
}

#[test]
fn twin_scan_skips_id_and_external_indexes() {
    let fixtures = vec![doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" }];
    let diff = compute_diff::<IdTwin>(&fixtures);
    assert!(diff.name_drift.is_empty(), "_id_ is not a drift target");
    assert!(diff.to_drop.is_empty(), "_id_ is never a drop candidate");
    assert_eq!(diff.to_create, ["by_key"]);
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "twins")]
#[index(by_key, keys(key))]
#[external_index(managed_elsewhere)]
pub struct ExternalTwin {
    pub key: String,
}

#[test]
fn declared_twin_of_an_external_index_is_name_drift() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! { "v": 2, "key": { "key": 1 }, "name": "managed_elsewhere" },
    ];
    let diff = compute_diff::<ExternalTwin>(&fixtures);
    assert_eq!(
        diff.name_drift,
        [NameDrift {
            expected: "by_key".to_string(),
            actual: "managed_elsewhere".to_string(),
        }]
    );
    assert!(diff.to_create.is_empty(), "the server would refuse it");
    assert!(diff.to_drop.is_empty());
    assert!(diff.missing_external.is_empty(), "the external is present");
    assert!(diff.fails_verify());
}

#[test]
fn absent_external_index_is_missing_external() {
    let mut fixtures = server_fixtures();
    fixtures.retain(|m| m.get_str("name").ok() != Some("legacy_geo"));
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.missing_external, ["legacy_geo"]);
    assert!(diff.fails_verify());
    assert!(
        diff.to_drop.is_empty() && diff.to_create.is_empty(),
        "external indexes are never created or dropped"
    );
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "cities")]
#[index(by_name, keys(name), collation(locale = "en"))]
pub struct City {
    pub name: String,
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "spellings")]
#[index(
    part,
    keys(total),
    partial_raw = "{\"status\": \"open\", \"total\": {\"$gt\": 10}}"
)]
#[index(flat, keys(location = 2d))]
pub struct Spelling {
    pub status: String,
    pub total: i64,
    pub location: Vec<f64>,
}

#[test]
fn equivalent_partial_spellings_and_2d_defaults_are_a_no_op() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "total": 1 },
            "name": "part",


            "partialFilterExpression": {
                "total": { "$gt": 10.0 },
                "status": { "$eq": "open" },
            },
        },
        doc! {
            "v": 2,
            "key": { "location": "2d" },
            "name": "flat",

            "bits": 26, "min": -180.0, "max": 180.0,
        },
    ];
    let diff = compute_diff::<Spelling>(&fixtures);
    assert_eq!(diff, IndexDiff::default());
}

#[test]
fn collation_strength_defaults_are_accepted_when_undeclared() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "name": 1 },
            "name": "by_name",
            "collation": {
                "locale": "en",
                "caseLevel": false,
                "caseFirst": "off",
                "strength": 3,
                "numericOrdering": false,
                "alternate": "non-ignorable",
                "maxVariable": "punct",
                "normalization": false,
                "backwards": false,
                "version": "57.1",
            },
        },
    ];
    let diff = compute_diff::<City>(&fixtures);
    assert_eq!(diff, IndexDiff::default());
}

#[test]
fn collation_strength_difference_is_mismatched() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "name": 1 },
            "name": "by_name",
            "collation": {
                "locale": "en",
                "caseLevel": false,
                "caseFirst": "off",
                "strength": 2,
                "numericOrdering": false,
                "alternate": "non-ignorable",
                "maxVariable": "punct",
                "normalization": false,
                "backwards": false,
                "version": "57.1",
            },
        },
    ];
    let diff = compute_diff::<City>(&fixtures);
    assert_eq!(diff.mismatched, vec!["by_name".to_string()]);
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "simple_cities")]
#[index(by_name, keys(name), collation(locale = "simple"))]
pub struct SimpleCity {
    pub name: String,
}

#[test]
fn simple_collation_compares_as_no_collation() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! { "v": 2, "key": { "name": 1 }, "name": "by_name" },
    ];
    let diff = compute_diff::<SimpleCity>(&fixtures);
    assert_eq!(diff, IndexDiff::default());

    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "name": 1 },
            "name": "by_name",
            "collation": { "locale": "simple" },
        },
    ];
    let diff = compute_diff::<SimpleCity>(&fixtures);
    assert_eq!(diff, IndexDiff::default());
}

#[test]
fn double_typed_ttl_seconds_still_matches() {
    let mut fixtures = server_fixtures();
    let expiry = fixtures
        .iter_mut()
        .find(|m| m.get_str("name") == Ok("expiry"))
        .unwrap();
    *expiry = doc! {
        "v": 2,
        "key": { "expires_at": 1 },
        "name": "expiry",
        "expireAfterSeconds": 3600.0,
    };
    let diff = compute_diff::<Order>(&fixtures);
    assert!(diff.is_empty(), "{diff:?}");
}

#[test]
fn unparseable_option_values_still_reach_the_diff() {
    let mut fixtures = server_fixtures();
    fixtures.push(doc! {
        "v": 2,
        "key": { "status": 1, "title": 1 },
        "name": "rogue_ttl",
        "expireAfterSeconds": -1,
    });
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.to_drop, ["rogue_ttl"]);

    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("expiry") {
            *m = doc! {
                "v": 2,
                "key": { "expires_at": 1 },
                "name": "expiry",
                "expireAfterSeconds": -1,
            };
        }
    }
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.mismatched, ["expiry"]);
    assert!(diff.fails_verify());

    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "status": 1 },
            "name": "plain",
            "expireAfterSeconds": -1,
        },
    ];
    let diff = compute_diff::<PlainIndexOrder>(&fixtures);
    assert_eq!(diff.mismatched, ["plain"]);
    assert!(diff.fails_verify());
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "plain_orders")]
#[index(plain, keys(status))]
pub struct PlainIndexOrder {
    pub status: String,
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "literal_partials")]
#[index(
    tagged,
    keys(total),
    partial_raw = "{\"tag\": {\"$in\": [{\"a\": 1, \"b\": 2}]}, \"$or\": [{\"status\": \"open\", \"total\": {\"$gt\": 1}}]}"
)]
pub struct LiteralPartial {
    pub status: String,
    pub total: i64,
}

fn literal_partial_fixture(in_literal: Document) -> Vec<Document> {
    vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "total": 1 },
            "name": "tagged",
            "partialFilterExpression": {


                "$or": [{ "total": { "$gt": 1 }, "status": { "$eq": "open" } }],
                "tag": { "$in": [in_literal] },
            },
        },
    ]
}

#[test]
fn in_literal_documents_compare_order_sensitively() {
    let diff = compute_diff::<LiteralPartial>(&literal_partial_fixture(doc! { "a": 1, "b": 2 }));
    assert_eq!(diff, IndexDiff::default());

    let diff = compute_diff::<LiteralPartial>(&literal_partial_fixture(doc! { "b": 2, "a": 1 }));
    assert_eq!(diff.mismatched, vec!["tagged".to_string()]);
}

#[test]
fn hidden_flag_differences_are_mismatched() {
    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("ghost") {
            *m = doc! { "v": 2, "key": { "body": 1 }, "name": "ghost" };
        }
    }
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.mismatched, ["ghost"]);

    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("sparse_title") {
            *m = doc! {
                "v": 2,
                "key": { "title": 1 },
                "name": "sparse_title",
                "sparse": true,
                "hidden": true,
            };
        }
    }
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.mismatched, ["sparse_title"]);
}

#[test]
fn ttl_value_difference_is_mismatched() {
    let mut fixtures = server_fixtures();
    for m in &mut fixtures {
        if m.get_str("name").ok() == Some("expiry") {
            *m = doc! {
                "v": 2,
                "key": { "expires_at": 1 },
                "name": "expiry",
                "expireAfterSeconds": 7200,
            };
        }
    }
    let diff = compute_diff::<Order>(&fixtures);
    assert_eq!(diff.mismatched, ["expiry"]);
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "scoped_orders")]
#[index(
    scoped_search,
    keys(tenant_id, title = text, body = text, created_at = -1),
    weights(title = 10)
)]
pub struct ScopedOrder {
    pub tenant_id: ObjectId,
    pub title: String,
    pub body: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
}

fn scoped_search_fixture(key: Document) -> Vec<Document> {
    vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": key,
            "name": "scoped_search",
            "weights": { "body": 1, "title": 10 },
            "default_language": "english",
            "language_override": "language",
            "textIndexVersion": 3,
        },
    ]
}

#[test]
fn text_block_with_prefix_and_suffix_keys_is_a_no_op() {
    let diff = compute_diff::<ScopedOrder>(&scoped_search_fixture(
        doc! { "tenant_id": 1, "_fts": "text", "_ftsx": 1, "createdAt": -1 },
    ));
    assert_eq!(diff, IndexDiff::default());
}

#[test]
fn text_block_with_relocated_suffix_key_is_mismatched() {
    let diff = compute_diff::<ScopedOrder>(&scoped_search_fixture(
        doc! { "tenant_id": 1, "createdAt": -1, "_fts": "text", "_ftsx": 1 },
    ));
    assert_eq!(diff.mismatched, ["scoped_search"]);
}

#[test]
fn declared_simple_collation_detects_a_collated_server_index() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2,
            "key": { "name": 1 },
            "name": "by_name",
            "collation": { "locale": "de", "strength": 3 },
        },
    ];
    let diff = compute_diff::<SimpleCity>(&fixtures);
    assert_eq!(diff.mismatched, vec!["by_name".to_string()], "{diff:?}");
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "plain_keys")]
#[index(by_key, keys(key))]
pub struct PlainKey {
    pub key: String,
}

#[test]
fn collated_twin_under_another_name_is_not_name_drift() {
    let fixtures = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! {
            "v": 2, "key": { "key": 1 }, "name": "key_fr",
            "collation": { "locale": "fr", "strength": 3, "caseLevel": false },
        },
    ];
    let diff = compute_diff::<PlainKey>(&fixtures);
    assert_eq!(diff.to_create, ["by_key"]);
    assert_eq!(diff.to_drop, ["key_fr"]);
    assert!(diff.name_drift.is_empty());

    let uncollated = vec![
        doc! { "v": 2, "key": { "_id": 1 }, "name": "_id_" },
        doc! { "v": 2, "key": { "key": 1 }, "name": "key_old" },
    ];
    let diff = compute_diff::<PlainKey>(&uncollated);
    assert_eq!(diff.name_drift.len(), 1);
    assert!(diff.to_create.is_empty());
}
