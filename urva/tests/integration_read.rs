mod common;

use common::{TestDb, index_names_in_plan};
use futures_util::TryStreamExt;
use mongodb::bson::{Bson, doc};
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "orders")]
#[index(by_tenant, keys(tenant_id, created_at = -1))]
pub struct Order {
    pub tenant_id: i64,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub total: i64,
}

use order_fields as o;
use order_index as ix;

fn order(tenant: i64, status: &str, millis: i64, total: i64) -> Order {
    Order {
        tenant_id: tenant,
        status: status.to_string(),
        created_at: DateTime::from_millis(millis),
        total,
    }
}

async fn seed(store: &Store<Order>) {
    store
        .insert_many([
            order(1, "open", 1_000, 10),
            order(1, "open", 2_000, 20),
            order(1, "packed", 3_000, 30),
            order(2, "open", 4_000, 40),
        ])
        .await
        .expect("seed");
}

#[tokio::test]
async fn reads_on_an_empty_collection() {
    let Some(t) = TestDb::connect("read_empty").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let by_id = || mongodb::options::Hint::Name("_id_".to_string());
    assert!(store.find(Filter::empty()).await.unwrap().is_empty());
    assert!(
        store
            .find(Filter::empty())
            .hint(by_id())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .find_one(o::status.eq("open"))
            .hint(by_id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.find_by_id(ObjectId::new()).await.unwrap().is_none());
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 0);
    assert_eq!(
        store
            .count_documents(Filter::empty())
            .hint(by_id())
            .await
            .unwrap(),
        0
    );
    assert_eq!(store.estimated_document_count().await.unwrap(), 0);
    let mut cursor = store.find(Filter::empty()).stream().await.unwrap();
    assert!(cursor.try_next().await.unwrap().is_none());

    t.drop().await;
}

#[tokio::test]
async fn read_cycle_with_serde_renames() {
    let Some(t) = TestDb::connect("read_cycle").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    seed(&store).await;

    let recent = store
        .find(all([
            o::tenant_id.eq(1),
            o::created_at.gte(DateTime::from_millis(1_500)),
        ]))
        .sort(o::created_at.desc())
        .await
        .unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].total, 30);
    assert_eq!(recent[1].total, 20);

    let one = store
        .find_one(o::status.eq("packed"))
        .await
        .unwrap()
        .expect("packed order");
    assert_eq!(one.total, 30);
    assert_eq!(one.created_at, DateTime::from_millis(3_000));
    assert_eq!(
        store.find_by_id(*one.id()).await.unwrap().expect("by id"),
        one
    );

    assert_eq!(
        store.count_documents(o::status.eq("open")).await.unwrap(),
        3
    );
    assert_eq!(store.estimated_document_count().await.unwrap(), 4);

    let page = store
        .find(o::tenant_id.eq(1))
        .sort(o::created_at.asc())
        .skip(1)
        .limit(1)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].total, 20);

    let mut cursor = store.find(Filter::empty()).stream().await.unwrap();
    let mut count = 0;
    while cursor.try_next().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 4);

    t.drop().await;
}

#[tokio::test]
async fn sort_after_with_options_replaces_the_whole_sort() {
    let Some(t) = TestDb::connect("sort_precedence").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    seed(&store).await;

    let opts = mongodb::options::FindOptions::builder()
        .sort(doc! { "status": 1 })
        .build();
    let rows = store
        .find(o::tenant_id.eq(1))
        .with_options(opts)
        .sort(o::total.desc())
        .await
        .unwrap();
    let totals: Vec<i64> = rows.iter().map(|r| r.total).collect();
    assert_eq!(
        totals,
        [30, 20, 10],
        "later .sort() replaced the options' sort"
    );

    let rows = store
        .find(o::tenant_id.eq(1))
        .sort(o::created_at.asc())
        .sort(o::total.desc())
        .await
        .unwrap();
    let totals: Vec<i64> = rows.iter().map(|r| r.total).collect();
    assert_eq!(
        totals,
        [30, 20, 10],
        "later .sort() replaced the earlier one"
    );

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "tickets")]
pub struct Ticket {
    pub state: TicketState,
    pub count: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum TicketState {
    Open,
    OnHold,
}

#[tokio::test]
async fn serde_only_values_match_stored_documents() {
    let Some(t) = TestDb::connect("enum_serde").await else {
        return;
    };
    let store: Store<Ticket> = t.db.store();
    use ticket_fields as tf;

    store
        .insert(Ticket {
            state: TicketState::OnHold,
            count: 3,
        })
        .await
        .expect("seed");
    store
        .insert(Ticket {
            state: TicketState::Open,
            count: 9,
        })
        .await
        .expect("seed");

    let found = store.find(tf::state.eq(TicketState::OnHold)).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].state, TicketState::OnHold);

    let found = store.find(tf::count.eq(3u32)).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].count, 3);

    store
        .update_one(tf::count.eq(9u32), tf::state.set(TicketState::OnHold))
        .await
        .unwrap();
    let n = store
        .count_documents(tf::state.eq(TicketState::OnHold))
        .await
        .unwrap();
    assert_eq!(n, 2);

    t.drop().await;
}

#[tokio::test]
async fn chained_and_stays_flat_for_the_server() {
    let Some(t) = TestDb::connect("read_deep_and").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    seed(&store).await;

    for n in [10usize, 120] {
        let mut chained = o::total.gte(0);
        let mut chained_or = o::total.lt(0);
        for i in 1..n {
            chained = chained.and(o::total.gte(-(i as i64)));
            chained_or = chained_or.or(o::total.lt(-(i as i64)));
        }
        let all_docs = store.find(chained).await.unwrap();
        assert_eq!(all_docs.len(), 4, "{n} chained .and() calls");
        let none = store.find(chained_or).await.unwrap();
        assert!(none.is_empty(), "{n} chained .or() calls");
    }

    t.drop().await;
}

#[tokio::test]
async fn symbol_hint_is_honored_by_the_server() {
    let Some(t) = TestDb::connect("hint_explain").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    seed(&store).await;
    store
        .raw()
        .create_indexes(Order::index_models())
        .await
        .expect("create indexes");

    let hinted = store
        .find(o::tenant_id.eq(1))
        .hint(ix::by_tenant)
        .await
        .unwrap();
    assert_eq!(hinted.len(), 3);

    let bogus = store
        .find(o::tenant_id.eq(1))
        .hint(mongodb::options::Hint::Name("does_not_exist".into()))
        .await;
    assert!(bogus.is_err(), "unknown hint names are a server error");

    let explain =
        t.db.run_command(doc! {
            "explain": {
                "find": Order::COLLECTION,
                "filter": { "status": { "$eq": "open" } },
                "hint": ix::by_tenant.name(),
            },
            "verbosity": "queryPlanner",
        })
        .await
        .unwrap();
    let names = index_names_in_plan(&Bson::Document(explain));
    assert!(
        names.iter().any(|n| n == "by_tenant"),
        "winning plan uses the hinted index; saw {names:?}"
    );

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "notes")]
#[index(note_search, keys(body = text))]
pub struct Note {
    pub body: String,
}

#[tokio::test]
async fn text_search_finds_indexed_documents() {
    let Some(t) = TestDb::connect("text_search").await else {
        return;
    };
    let store: Store<Note> = t.db.store();
    store.create_indexes().await.unwrap();
    let a = store
        .insert(Note {
            body: "red kayak on the river".into(),
        })
        .await
        .unwrap();
    store
        .insert(Note {
            body: "blue bicycle in the shed".into(),
        })
        .await
        .unwrap();

    let hits = store.find(text("kayak")).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id(), a.id());

    let none = store.find(text("submarine")).await.unwrap();
    assert!(none.is_empty());

    t.drop().await;
}
