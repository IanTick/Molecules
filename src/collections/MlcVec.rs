#[deny(clippy::pedantic)]
use crate::primitives::AtomicCell::*;
use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub struct MlcVec<T> {
    pub (crate) beam: AtomicCell<Vec<Arc<AtomicCell<T>>>>,
}

impl<T> MlcVec<T> {
    pub fn new() -> Self {
        Self {
            beam: AtomicCell::new(Vec::new()),
        }
    }

    pub fn from<O>(mut iterable: O) -> Self
    where
        O: Iterator<Item = T>,
    {
        let mut vector = Vec::new();
        while let Some(element) = iterable.next() {
            vector.push(Arc::new(AtomicCell::new(element)));
        }
        Self {
            beam: AtomicCell::new(vector),
        }
    }

    pub fn get(&self, idx: usize) -> Option<Arc<AtomicCell<T>>> {
        Some((*self.beam.load()).get(idx)?.clone())
    }

    pub fn push(&self, value: T) {
        let mut elem = Arc::new(AtomicCell::new(value));

        loop {
            // Get Current vec cloned. (Double load)
            // Change current vec,
            // Cas
            let (vec, ptr) = self.beam.double_load();
            let mut owned_vec = (*vec).clone();
            owned_vec.push(elem.clone());

            unsafe {
                match self.beam.cas(ptr, owned_vec) {
                    Ok(_) => break,
                    Err(_) => continue,
                }
            }
        }
    }

    /*pub fn remove(&self, ) {
        loop {
            // Get Current vec cloned. (Double load)
            // Change current vec,
            // Cas
            let (vec, ptr) = self.beam.double_load();
            let mut owned_vec = (*vec).clone();
            owned_vec.remove(idx);
        }
    }*/ 
    pub fn pop(){}
}
