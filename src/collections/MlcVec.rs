#[deny(clippy::pedantic)]
use crate::primitives::AtomicCell::*;
use std::sync::Arc;


// TODO: Add Iterator support
pub struct MlcVec<T> {
    pub(crate) beam: AtomicCell<Vec<Arc<T>>>,
}

impl<T> MlcVec<T> {
    pub fn new() -> Self {
        Self {
            beam: AtomicCell::new(Vec::new()),
        }
    }

    // MlcVec does not expose a write handle to individual T's. Use Wrappers such as AtomicCell or Mutex to modify through shared references.
    // This enables using only one wrapper for better matrices: MlcVec<MlcVec<Wrapper<T>>>
    pub fn get(&self, idx: usize) -> Option<Arc<T>> {
        match self.beam.load().get(idx) {
            Some(z) => Some(z.clone()),
            None => None,
        }
    }

    pub fn push(&self, data: T) {
        let new = Arc::new(data);
        // Assume clone does not panic
        let _ = self.beam.fetch_update::<(), _>(|vec| {
            let mut next_vec = (*vec).clone();
            next_vec.push(new.clone());
            (Arc::new(next_vec), ())
    });
    }

    // TODO: Add pop for any index
    pub fn pop(&self) -> Option<Arc<T>> {
        self.beam.fetch_update::<Option<Arc<T>>, _>(|vec| {
            let mut next_vec = (*vec).clone();
            let output = next_vec.pop();
            (Arc::new(next_vec), output)
        })
        // Assume clone does not panic
        .unwrap()
    }
}
