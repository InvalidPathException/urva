use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub tenant_id: ObjectId,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub total: i64,
}

#[test]
fn entity_contract_constants() {
    assert_eq!(Order::COLLECTION, "orders");
    assert_eq!(Order::VERSION_FIELD, "version");
    assert_versioned::<Order>();
    assert_id::<Order, ObjectId>();
    assert_generates_ids::<Order>();
}

fn assert_versioned<E: urva::Versioned>() {}
fn assert_unversioned<E: urva::Unversioned>() {}
fn assert_id<E: Entity<Id = I>, I>() {}
fn assert_generates_ids<E: Entity>()
where
    E::Id: urva::NewId,
{
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "notes", id = String)]
pub struct Note {
    pub text: String,
}

#[test]
fn declared_id_type_and_unversioned_lock() {
    assert_eq!(Note::COLLECTION, "notes");
    assert_unversioned::<Note>();
    assert_id::<Note, String>();
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "revisions", versioned = "rev")]
pub struct Revision {
    pub text: String,
}

#[test]
fn renamed_version_field_is_the_stored_lock_name() {
    assert_eq!(Revision::VERSION_FIELD, "rev");
    assert_versioned::<Revision>();
}

#[test]
fn entities_declared_inside_a_function_body_compile() {
    #[derive(Embedded, Serialize, Deserialize, Debug)]
    struct Local {
        n: i64,
    }
    #[derive(Entity, Serialize, Deserialize, Debug)]
    #[entity(collection = "scoped")]
    struct Scoped {
        local: Local,
    }
    assert_eq!(Scoped::COLLECTION, "scoped");
    assert_unversioned::<Scoped>();
    let _ = Scoped {
        local: Local { n: 1 },
    };
}
