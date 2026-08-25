mod common;

use common::TestDb;
use urva::prelude::*;

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
    let mut docs = Vec::new();
    for body in bodies {
        docs.push(store.insert(body).await.unwrap());
    }

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
    for doc in &docs {
        store.insert(doc.clone()).await.unwrap();
    }

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
