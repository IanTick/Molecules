use crate::primitives::AtomicCell::*;
#[deny(clippy::pedantic)]
use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub struct MlcVec<T> {
    beam: AtomicCell<Vec<Arc<AtomicCell<T>>>>,
}

impl<T> MlcVec<T> {
    fn new() -> Self {
        Self {
            beam: AtomicCell::new(Vec::new()),
        }
    }

    fn from(iterable: T) -> Self
    where
        T: Iterator,
    {
        let mut vector = Vec::new();
        while let Some(element) = iterable.next() {
            vector.push(Arc::new(AtomicCell::new(element)));
        }
    }

    fn load(&self, idx: usize) -> Option<Arc<T>> {
        Some(self.beam.load().as_ref().get(idx)?.load())
    }
}
