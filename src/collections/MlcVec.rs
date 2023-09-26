#[deny(clippy::pedantic)]
use crate::primitives::AtomicCell::*;
use std::sync::Arc;

pub struct MlcVec<T> {
    pub(crate) beam: AtomicCell<Vec<Arc<T>>>,
}

impl<T> MlcVec<T> {
    pub fn new() -> Self {
        Self {
            beam: AtomicCell::new(Vec::new()),
        }
    }

    pub fn get(&self, idx: usize) -> Option<Arc<T>> {
        match self.beam.load().get(idx) {
            Some(z) => Some(z.clone()),
            None => None,
        }
    }

    pub fn push(&self, data: T) {

        let new = Arc::new(data);
        self.beam.fetch_update::<T, _>(|vec| {
            let mut next_vec = (*vec).clone();
            next_vec.push(new.clone());
            (next_vec, None)
        });
    }

    pub fn pop(&self) -> Option<Arc<T>> {
        self.beam.fetch_update(|vec| {
            let mut next_vec = (*vec).clone();
            let output = next_vec.pop();
            (next_vec, Some(output))
        }).unwrap();
    }
}
