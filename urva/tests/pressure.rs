mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::TestDb;
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "counters", versioned)]
pub struct Counter {
    pub total: i64,
}

#[tokio::test]
async fn save_race_counter_reaches_exact_total() {
    const TASKS: usize = 16;
    const INCREMENTS: usize = 25;

    let Some(t) = TestDb::connect("pressure_save_race").await else {
        return;
    };
    let store: Store<Counter> = t.db.store();
    let id = *store.insert(Counter { total: 0 }).await.unwrap().id();

    let conflicts = Arc::new(AtomicU32::new(0));
    let mut tasks = Vec::new();
    for _ in 0..TASKS {
        let store = store.clone();
        let conflicts = conflicts.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..INCREMENTS {
                let mut attempts = 0u32;
                loop {
                    let mut current = store.find_by_id(id).await.unwrap().expect("counter exists");
                    current.total += 1;
                    match store.save(&mut current).await {
                        Ok(()) => break,
                        Err(Error::VersionConflict { .. }) => {
                            conflicts.fetch_add(1, Ordering::SeqCst);
                            attempts += 1;
                            assert!(attempts < 10_000, "retry loop did not converge");
                        }
                        Err(other) => panic!("unexpected save error: {other:?}"),
                    }
                }
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    let after = store.find_by_id(id).await.unwrap().unwrap();
    assert_eq!(after.total, (TASKS * INCREMENTS) as i64, "no lost update");
    assert_eq!(
        after.version().value(),
        (TASKS * INCREMENTS) as i64 + 1,
        "version advanced by exactly 1 per successful save"
    );
    let conflicts = conflicts.load(Ordering::SeqCst);
    assert!(
        conflicts >= 1,
        "contention was real (conflicts: {conflicts})"
    );

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "adversarial")]
pub struct Sample {
    pub tag: String,
    pub n: i64,
    pub items: Vec<String>,
    pub blob: String,
}

use sample_fields as s;

fn sample(tag: &str, n: i64, items: Vec<String>) -> Sample {
    Sample {
        tag: tag.to_string(),
        n,
        items,
        blob: String::new(),
    }
}

#[tokio::test]
async fn adversarial_values_read_back_through_filters() {
    let Some(t) = TestDb::connect("pressure_adversarial").await else {
        return;
    };
    let store: Store<Sample> = t.db.store();

    let tags: Vec<String> = vec![
        "$gt".to_string(),
        "$where".to_string(),
        "a.b.c".to_string(),
        "{\"$set\": {\"n\": 0}}".to_string(),
        "🦀🚀🧨".to_string(),
        "مرحبا بالعالم".to_string(),
        "\u{00e9}clair".to_string(),
        "e\u{0301}clair".to_string(),
        "line\nbreak\ttab".to_string(),
        "back\\slash \"quotes\"".to_string(),
    ];
    let mut bodies: Vec<Sample> = tags
        .iter()
        .enumerate()
        .map(|(i, tag)| {
            sample(
                tag,
                i as i64,
                vec!["$dollar".to_string(), "dot.ted".to_string()],
            )
        })
        .collect();
    bodies.push(sample("", i64::MIN, Vec::new()));
    bodies.push(sample("max", i64::MAX, vec![String::new()]));
    let docs = store.insert_many(bodies).await.unwrap();

    for doc in &docs {
        let found = store.find(s::tag.eq(doc.tag.clone())).await.unwrap();
        assert_eq!(found.len(), 1, "tag {:?} matches exactly one", doc.tag);
        assert_eq!(&found[0], doc, "write-then-read equality for {:?}", doc.tag);
    }

    let min = store.find(s::n.eq(i64::MIN)).await.unwrap();
    assert_eq!(min.len(), 1);
    assert_eq!(min[0].tag, "");
    let max = store.find(s::n.gte(i64::MAX)).await.unwrap();
    assert_eq!(max.len(), 1);
    assert_eq!(max[0].tag, "max");
    let all_but_min = store.find(s::n.gt(i64::MIN)).await.unwrap();
    assert_eq!(all_but_min.len(), docs.len() - 1);

    let empty_items = store.find(s::items.size(0)).await.unwrap();
    assert_eq!(empty_items.len(), 1);
    assert_eq!(empty_items[0].n, i64::MIN);
    let dollar = store.find(s::items.contains("$dollar")).await.unwrap();
    assert_eq!(dollar.len(), tags.len(), "every doc with the $ element");
    let dotted = store.find(s::items.contains("dot.ted")).await.unwrap();
    assert_eq!(dotted.len(), tags.len());

    t.drop().await;
}

#[tokio::test]
async fn near_limit_document_reads_back_intact() {
    let Some(t) = TestDb::connect("pressure_large_doc").await else {
        return;
    };
    let store: Store<Sample> = t.db.store();

    let mut big = sample("big", 1, Vec::new());
    big.blob = "x".repeat(15_500_000);
    let big = store.insert(big).await.unwrap();

    let read = store
        .find_one(s::_id.eq(big.id()))
        .await
        .unwrap()
        .expect("large document present");
    assert!(read.blob == big.blob, "15.5MB blob read back intact");
    assert_eq!(read.tag, "big");

    t.drop().await;
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "fuzz")]
pub struct FuzzDoc {
    pub k: i64,
    pub a: i64,
    pub b: i64,
    pub tag: String,
}

use fuzz_doc_fields as fz;

const FUZZ_TAGS: [&str; 4] = ["red", "green", "blue", "🦀"];

#[derive(Debug, Clone)]
enum Node {
    AEq(i64),
    AGt(i64),
    ALte(i64),
    ANe(i64),
    AIn(Vec<i64>),
    BLt(i64),
    BGte(i64),
    TagEq(&'static str),
    All(Vec<Node>),
    Any(Vec<Node>),
}

fn gen_a(rng: &mut Lcg) -> i64 {
    rng.below(40) as i64 - 20
}

fn gen_b(rng: &mut Lcg) -> i64 {
    rng.below(50) as i64
}

fn gen_node(rng: &mut Lcg, depth: u32) -> Node {
    if depth == 0 || rng.below(3) == 0 {
        match rng.below(8) {
            0 => Node::AEq(gen_a(rng)),
            1 => Node::AGt(gen_a(rng)),
            2 => Node::ALte(gen_a(rng)),
            3 => Node::ANe(gen_a(rng)),
            4 => Node::AIn((0..1 + rng.below(3)).map(|_| gen_a(rng)).collect()),
            5 => Node::BLt(gen_b(rng)),
            6 => Node::BGte(gen_b(rng)),
            _ => Node::TagEq(FUZZ_TAGS[rng.below(4) as usize]),
        }
    } else {
        let children: Vec<Node> = (0..2 + rng.below(2))
            .map(|_| gen_node(rng, depth - 1))
            .collect();
        if rng.below(2) == 0 {
            Node::All(children)
        } else {
            Node::Any(children)
        }
    }
}

fn to_filter(node: &Node) -> Filter<FuzzDoc> {
    match node {
        Node::AEq(v) => fz::a.eq(*v),
        Node::AGt(v) => fz::a.gt(*v),
        Node::ALte(v) => fz::a.lte(*v),
        Node::ANe(v) => fz::a.ne(*v),
        Node::AIn(vs) => fz::a.is_in(vs.clone()),
        Node::BLt(v) => fz::b.lt(*v),
        Node::BGte(v) => fz::b.gte(*v),
        Node::TagEq(tag) => fz::tag.eq(*tag),
        Node::All(children) => all(children.iter().map(to_filter)),
        Node::Any(children) => any(children.iter().map(to_filter)),
    }
}

fn eval(node: &Node, d: &FuzzDoc) -> bool {
    match node {
        Node::AEq(v) => d.a == *v,
        Node::AGt(v) => d.a > *v,
        Node::ALte(v) => d.a <= *v,
        Node::ANe(v) => d.a != *v,
        Node::AIn(vs) => vs.contains(&d.a),
        Node::BLt(v) => d.b < *v,
        Node::BGte(v) => d.b >= *v,
        Node::TagEq(tag) => d.tag == *tag,
        Node::All(children) => children.iter().all(|n| eval(n, d)),
        Node::Any(children) => children.iter().any(|n| eval(n, d)),
    }
}

#[tokio::test]
async fn filter_fuzz_matches_in_memory_model() {
    let Some(t) = TestDb::connect("pressure_fuzz").await else {
        return;
    };
    let store: Store<FuzzDoc> = t.db.store();

    let mut rng = Lcg(0x5eed_2026_0831);
    let docs: Vec<FuzzDoc> = (0..200)
        .map(|k| FuzzDoc {
            k,
            a: gen_a(&mut rng),
            b: gen_b(&mut rng),
            tag: FUZZ_TAGS[rng.below(4) as usize].to_string(),
        })
        .collect();
    store.insert_many(docs.iter().cloned()).await.unwrap();

    for trial in 0..60 {
        let node = gen_node(&mut rng, 2);
        let mut expected: Vec<i64> = docs
            .iter()
            .filter(|d| eval(&node, d))
            .map(|d| d.k)
            .collect();
        expected.sort_unstable();
        let mut actual: Vec<i64> = store
            .find(to_filter(&node))
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.k)
            .collect();
        actual.sort_unstable();
        assert_eq!(
            actual, expected,
            "trial {trial}: server disagrees with the model for {node:?}"
        );

        let n = store.count_documents(to_filter(&node)).await.unwrap();
        assert_eq!(
            n as usize,
            expected.len(),
            "trial {trial}: count for {node:?}"
        );
    }

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "probe")]
pub struct Probe {
    pub a: i64,
    pub b: i64,
    pub c: i64,
    pub tags: Vec<String>,
}

use probe_fields as p;

fn probe() -> Probe {
    Probe {
        a: 0,
        b: 0,
        c: 0,
        tags: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Set,
    Inc,
    Unset,
    Min,
    Max,
    Push,
    AddToSet,
    PopLast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Path {
    A,
    B,
    C,
    Tags,
}

fn render(op: Op, path: Path, v: i64) -> Update<Probe> {
    match (path, op) {
        (Path::A, Op::Set) => p::a.set(v),
        (Path::A, Op::Inc) => p::a.inc(v),
        (Path::A, Op::Unset) => p::a.unset(),
        (Path::A, Op::Min) => p::a.min(v),
        (Path::A, Op::Max) => p::a.max(v),
        (Path::B, Op::Set) => p::b.set(v),
        (Path::B, Op::Inc) => p::b.inc(v),
        (Path::B, Op::Unset) => p::b.unset(),
        (Path::B, Op::Min) => p::b.min(v),
        (Path::B, Op::Max) => p::b.max(v),
        (Path::C, Op::Set) => p::c.set(v),
        (Path::C, Op::Inc) => p::c.inc(v),
        (Path::C, Op::Unset) => p::c.unset(),
        (Path::C, Op::Min) => p::c.min(v),
        (Path::C, Op::Max) => p::c.max(v),
        (Path::Tags, Op::Push) => p::tags.push(v.to_string()),
        (Path::Tags, Op::AddToSet) => p::tags.add_to_set(v.to_string()),
        (Path::Tags, Op::PopLast) => p::tags.pop_last(),
        (Path::Tags, Op::Unset) => p::tags.unset(),
        (Path::Tags, _) | (_, Op::Push | Op::AddToSet | Op::PopLast) => unreachable!(),
    }
}

fn gen_op(rng: &mut Lcg) -> (Op, Path, i64) {
    let path = match rng.below(4) {
        0 => Path::A,
        1 => Path::B,
        2 => Path::C,
        _ => Path::Tags,
    };
    let op = if path == Path::Tags {
        match rng.below(4) {
            0 => Op::Push,
            1 => Op::AddToSet,
            2 => Op::PopLast,
            _ => Op::Unset,
        }
    } else {
        match rng.below(5) {
            0 => Op::Set,
            1 => Op::Inc,
            2 => Op::Unset,
            3 => Op::Min,
            _ => Op::Max,
        }
    };
    (op, path, rng.below(20) as i64 - 10)
}

#[tokio::test]
async fn update_composition_fuzz_agrees_with_the_server() {
    let Some(t) = TestDb::connect("review_update_fuzz").await else {
        return;
    };
    let store: Store<Probe> = t.db.store();
    let id = *store.insert(probe()).await.unwrap().id();

    let mut rng = Lcg(0xfeed_beef);
    let (mut ok, mut ours, mut theirs) = (0, 0, 0);
    for trial in 0..250 {
        let n = 1 + rng.below(4) as usize;
        let ops: Vec<(Op, Path, i64)> = (0..n).map(|_| gen_op(&mut rng)).collect();
        let repeated_pair = ops
            .iter()
            .enumerate()
            .any(|(i, (op, path, _))| ops[..i].iter().any(|(o, q, _)| o == op && q == path));
        let repeated_path = ops
            .iter()
            .enumerate()
            .any(|(i, (_, path, _))| ops[..i].iter().any(|(_, q, _)| q == path));

        let mut update = render(ops[0].0, ops[0].1, ops[0].2);
        for (op, path, v) in &ops[1..] {
            update = update.and(render(*op, *path, *v));
        }
        let result = store.update_one(p::_id.eq(id), update).await;
        match result {
            Ok(_) => {
                ok += 1;
                assert!(
                    !repeated_path,
                    "trial {trial}: server accepted a repeated path: {ops:?}"
                );
            }
            Err(Error::InvalidUpdate { message }) => {
                ours += 1;
                assert!(
                    repeated_pair,
                    "trial {trial}: InvalidUpdate without a repeated (op, path): {ops:?}: {message}"
                );
            }
            Err(Error::Driver(err)) => {
                theirs += 1;
                assert!(
                    repeated_path && !repeated_pair,
                    "trial {trial}: driver error for {ops:?}: {err}"
                );
                let code = match &*err.kind {
                    mongodb::error::ErrorKind::Write(mongodb::error::WriteFailure::WriteError(
                        w,
                    )) => w.code,
                    other => panic!("trial {trial}: unexpected error kind {other:?}"),
                };
                assert_eq!(
                    code, 40,
                    "trial {trial}: ConflictingUpdateOperators for {ops:?}"
                );
            }
            Err(other) => panic!("trial {trial}: {other:?}"),
        }
    }
    eprintln!("update fuzz: ok={ok} ours={ours} theirs={theirs}");
    assert!(
        ok > 0 && ours > 0 && theirs > 0,
        "every class was exercised"
    );

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "slots", versioned)]
pub struct Slot {
    pub key: i64,
    pub total: i64,
}

fn slot(key: i64) -> Slot {
    Slot { key, total: 0 }
}

async fn unique_key_index(store: &Store<Slot>) {
    store
        .raw()
        .create_index(
            mongodb::IndexModel::builder()
                .keys(mongodb::bson::doc! { "key": 1 })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .unique(true)
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();
}

const BATCH_BOUNDARY: usize = 100_000;

fn indices<T>(entries: &[(usize, T)]) -> Vec<usize> {
    entries.iter().map(|(i, _)| *i).collect()
}

fn dup_positions(total: usize) -> Vec<usize> {
    (0..total).filter(|i| i % 25_000 == 7).collect()
}

#[tokio::test]
async fn insert_many_failure_indices_are_global_across_driver_batches() {
    const TOTAL: usize = BATCH_BOUNDARY + 20_000;
    let Some(t) = TestDb::connect("review_insert_many_batches").await else {
        return;
    };
    let store: Store<Slot> = t.db.store();
    unique_key_index(&store).await;
    let seeds = dup_positions(TOTAL).into_iter().map(|i| slot(i as i64));
    store.insert_many(seeds).await.unwrap();

    let failure = store
        .insert_many((0..TOTAL).map(|i| slot(i as i64)))
        .ordered(false)
        .partial()
        .await
        .expect_err("collisions");
    assert!(failure.error.is_duplicate_key(), "{:?}", failure.error);
    assert_eq!(
        indices(&failure.inserted),
        (0..TOTAL).filter(|i| i % 25_000 != 7).collect::<Vec<_>>(),
        "unordered: inserted = complement, global indices"
    );
    assert_eq!(indices(&failure.rejected), dup_positions(TOTAL));
    for (i, doc) in &failure.inserted {
        assert_eq!(doc.key, *i as i64, "inserted doc at {i} is the queued body");
    }
    for (i, body) in &failure.rejected {
        assert_eq!(
            body.key, *i as i64,
            "rejected body at {i} is the queued body"
        );
    }
    assert_eq!(
        store.count_documents(Filter::empty()).await.unwrap() as usize,
        TOTAL
    );

    t.db.drop().await.unwrap();
    unique_key_index(&store).await;
    let first_dup = BATCH_BOUNDARY + 7;
    store.insert(slot(first_dup as i64)).await.unwrap();
    let failure = store
        .insert_many((0..TOTAL).map(|i| slot(i as i64)))
        .ordered(true)
        .partial()
        .await
        .expect_err("collision");
    assert!(failure.error.is_duplicate_key(), "{:?}", failure.error);
    assert_eq!(
        indices(&failure.inserted),
        (0..first_dup).collect::<Vec<_>>(),
        "ordered: everything before the collision landed"
    );
    assert_eq!(
        indices(&failure.rejected),
        (first_dup..TOTAL).collect::<Vec<_>>(),
        "ordered: the collision and everything after it came back"
    );
    assert_eq!(
        store.count_documents(Filter::empty()).await.unwrap() as usize,
        first_dup + 1
    );

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "bags")]
pub struct Bag {
    pub k: i64,
    pub items: Vec<Bin>,
}

#[derive(Embedded, Serialize, Deserialize, Debug, Clone)]
pub struct Bin {
    pub q: i64,
    pub s: String,
}

use bag_fields as bg;
use bin_fields as it;

const S: [&str; 3] = ["x", "y", "z"];

#[derive(Debug, Clone)]
enum BagNode {
    AnyQGt(i64),
    AnySEq(&'static str),
    ElemBoth(i64, &'static str),
    ElemEither(i64, &'static str),
    Size(usize),
    All(Vec<BagNode>),
    Any(Vec<BagNode>),
}

fn gen_bag_node(rng: &mut Lcg, depth: u32) -> BagNode {
    if depth == 0 || rng.below(3) == 0 {
        match rng.below(5) {
            0 => BagNode::AnyQGt(rng.below(10) as i64),
            1 => BagNode::AnySEq(S[rng.below(3) as usize]),
            2 => BagNode::ElemBoth(rng.below(10) as i64, S[rng.below(3) as usize]),
            3 => BagNode::ElemEither(rng.below(10) as i64, S[rng.below(3) as usize]),
            _ => BagNode::Size(rng.below(4) as usize),
        }
    } else {
        let children: Vec<BagNode> = (0..2 + rng.below(2))
            .map(|_| gen_bag_node(rng, depth - 1))
            .collect();
        if rng.below(2) == 0 {
            BagNode::All(children)
        } else {
            BagNode::Any(children)
        }
    }
}

fn bag_filter(node: &BagNode) -> Filter<Bag> {
    match node {
        BagNode::AnyQGt(v) => bg::items.dot(it::q).gt(*v),
        BagNode::AnySEq(s) => bg::items.dot(it::s).eq(*s),
        BagNode::ElemBoth(v, s) => bg::items.elem_match(all([it::q.gt(*v), it::s.eq(*s)])),
        BagNode::ElemEither(v, s) => bg::items.elem_match(any([it::q.gt(*v), it::s.eq(*s)])),
        BagNode::Size(n) => bg::items.size(*n as u32),
        BagNode::All(children) => all(children.iter().map(bag_filter)),
        BagNode::Any(children) => any(children.iter().map(bag_filter)),
    }
}

fn bag_eval(node: &BagNode, d: &Bag) -> bool {
    match node {
        BagNode::AnyQGt(v) => d.items.iter().any(|i| i.q > *v),
        BagNode::AnySEq(s) => d.items.iter().any(|i| i.s == *s),
        BagNode::ElemBoth(v, s) => d.items.iter().any(|i| i.q > *v && i.s == *s),
        BagNode::ElemEither(v, s) => d.items.iter().any(|i| i.q > *v || i.s == *s),
        BagNode::Size(n) => d.items.len() == *n,
        BagNode::All(children) => children.iter().all(|n| bag_eval(n, d)),
        BagNode::Any(children) => children.iter().any(|n| bag_eval(n, d)),
    }
}

#[tokio::test]
async fn array_filter_fuzz_matches_in_memory_model() {
    let Some(t) = TestDb::connect("review_array_fuzz").await else {
        return;
    };
    let store: Store<Bag> = t.db.store();
    let mut rng = Lcg(0xabcdef);
    let docs: Vec<Bag> = (0..150)
        .map(|k| Bag {
            k,
            items: (0..rng.below(4))
                .map(|_| Bin {
                    q: rng.below(10) as i64,
                    s: S[rng.below(3) as usize].to_string(),
                })
                .collect(),
        })
        .collect();
    store.insert_many(docs.iter().cloned()).await.unwrap();

    for trial in 0..80 {
        let node = gen_bag_node(&mut rng, 2);
        let mut expected: Vec<i64> = docs
            .iter()
            .filter(|d| bag_eval(&node, d))
            .map(|d| d.k)
            .collect();
        expected.sort_unstable();
        let mut actual: Vec<i64> = store
            .find(bag_filter(&node))
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.k)
            .collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "trial {trial}: {node:?}");
    }

    t.drop().await;
}
