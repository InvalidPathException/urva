use std::marker::PhantomData;

use mongodb::bson::{Bson, Document};

use crate::field::{Field, Filterable};

pub struct Sort<E: ?Sized> {
    doc: Document,
    _marker: PhantomData<fn() -> Box<E>>,
}

impl<E: ?Sized> Sort<E> {
    fn single(path: &str, direction: i32) -> Self {
        let mut doc = Document::new();
        doc.insert(path, Bson::Int32(direction));
        Sort {
            doc,
            _marker: PhantomData,
        }
    }

    pub fn then(mut self, next: Sort<E>) -> Sort<E> {
        for (key, value) in next.doc {
            self.doc.remove(&key);
            self.doc.insert(key, value);
        }
        self
    }

    #[doc(hidden)]
    pub fn into_document(self) -> Document {
        self.doc
    }
}

impl<E: ?Sized> Clone for Sort<E> {
    fn clone(&self) -> Self {
        Sort {
            doc: self.doc.clone(),
            _marker: PhantomData,
        }
    }
}

impl<E: ?Sized> std::fmt::Debug for Sort<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Sort").field(&self.doc).finish()
    }
}

impl<E: ?Sized, T: ?Sized, C: Filterable, X> Field<E, T, C, X> {
    pub fn asc(self) -> Sort<E> {
        Sort::single(self.path(), 1)
    }

    pub fn desc(self) -> Sort<E> {
        Sort::single(self.path(), -1)
    }
}
