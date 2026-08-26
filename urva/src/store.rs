use mongodb::{Collection, Database};

use crate::doc::Doc;
use crate::entity::Entity;
use crate::filter::{FieldValue, Filter};
use crate::ops::ById;
use crate::ops::count::{CountBuilder, EstimatedCountBuilder};
use crate::ops::delete::{DeleteManyBuilder, DeleteOneBuilder};
use crate::ops::find::{FindBuilder, FindOneBuilder};
use crate::ops::update::{UpdateManyBuilder, UpdateOneBuilder};
use crate::update::Update;

pub struct Store<E: Entity> {
    collection: Collection<Doc<E>>,
}

impl<E: Entity> Clone for Store<E> {
    fn clone(&self) -> Self {
        Store {
            collection: self.collection.clone(),
        }
    }
}

impl<E: Entity> Store<E> {
    pub fn raw(&self) -> &Collection<Doc<E>> {
        &self.collection
    }

    pub fn find(&self, filter: Filter<E>) -> FindBuilder<E> {
        FindBuilder::new(self.collection.clone(), filter)
    }

    pub fn find_one(&self, filter: Filter<E>) -> FindOneBuilder<E> {
        FindOneBuilder::new(self.collection.clone(), filter)
    }

    pub fn find_by_id(&self, id: impl FieldValue<E::Id>) -> FindOneBuilder<E, ById> {
        FindOneBuilder::new(self.collection.clone(), id_filter(id))
    }

    pub fn count_documents(&self, filter: Filter<E>) -> CountBuilder<E> {
        CountBuilder::new(self.collection.clone(), filter)
    }

    pub fn estimated_document_count(&self) -> EstimatedCountBuilder<E> {
        EstimatedCountBuilder::new(self.collection.clone())
    }

    pub fn update_one(&self, filter: Filter<E>, update: Update<E>) -> UpdateOneBuilder<E> {
        UpdateOneBuilder::new(self.collection.clone(), filter, update)
    }

    pub fn update_by_id(
        &self,
        id: impl FieldValue<E::Id>,
        update: Update<E>,
    ) -> UpdateOneBuilder<E, ById> {
        UpdateOneBuilder::new(self.collection.clone(), id_filter(id), update)
    }

    pub fn update_many(&self, filter: Filter<E>, update: Update<E>) -> UpdateManyBuilder<E> {
        UpdateManyBuilder::new(self.collection.clone(), filter, update)
    }

    pub fn delete_one(&self, filter: Filter<E>) -> DeleteOneBuilder<E> {
        DeleteOneBuilder::new(self.collection.clone(), filter)
    }

    pub fn delete_by_id(&self, id: impl FieldValue<E::Id>) -> DeleteOneBuilder<E, ById> {
        DeleteOneBuilder::new(self.collection.clone(), id_filter(id))
    }

    pub fn delete_many(&self, filter: Filter<E>) -> DeleteManyBuilder<E> {
        DeleteManyBuilder::new(self.collection.clone(), filter)
    }
}

pub(crate) fn id_filter<E: Entity>(id: impl FieldValue<E::Id>) -> Filter<E> {
    Filter::field_op("_id", "$eq", id.encode_value())
}

pub trait StoreExt {
    fn store<E: Entity>(&self) -> Store<E>;
}

impl StoreExt for Database {
    fn store<E: Entity>(&self) -> Store<E> {
        Store {
            collection: self.collection(E::COLLECTION),
        }
    }
}
