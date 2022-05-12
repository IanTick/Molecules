use crate::primitives::AtomicCell::*;
#[deny(clippy::pedantic)]
use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
// ctries or concurrent hamt
pub struct MlcMap<K, V> {
    is_creator: bool,
    bucket_vec: Arc<AtomicCell<Vec<Bucket<K, V>>>>,
}

impl<K, V> MlcMap<K, V> {
    fn get_hash(&self, key: &K) -> usize
    where
        K: Hash + Eq,
    {
        let mut hasher_instance = DefaultHasher::new();
        key.hash(&mut hasher_instance);
        hasher_instance.finish() as usize
    }

    fn new() -> Self {
        Self {
            is_creator: true,
            bucket_vec: Arc::new(AtomicCell::new(Vec::new())),
        }
    }

    pub fn new_with_capacity(capacity: usize) -> Self {
        let mut vector = Vec::with_capacity(capacity);

        for _ in 0..capacity {
            vector.push(Bucket::new());
        }

        Self {
            is_creator: true,
            bucket_vec: Arc::new(AtomicCell::new(vector)),
        }
    }

    pub fn get_handle(&self, key: &K) -> Option<Arc<(K, AtomicCell<V>)>>
    where
        K: Hash + Eq,
    {
        let hash = self.get_hash(key);
        let curr_map = self.bucket_vec.load();
        let spot = hash.checked_rem((*curr_map).len())?;
        (*curr_map).get(spot)?.get_handle(key)
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>>
    where
        K: Hash + Eq,
    {
        Some((*(self.get_handle(key)?)).1.load())
    }

    pub fn get_owned(&self, key: &K) -> Option<V>
    where
        K: Hash + Eq,
        V: Clone,
    {
        let arc = self.get(key)?;
        Some((*arc).clone())
    }

    pub fn edit(&self, key: &K, value: V) -> Option<()>
    where
        K: Hash + Eq,
    {
        (*(self.get_handle(key)?)).1.store(value);
        Some(())
    }

    pub fn insert(&self, key: K, value: V) -> Option<()>
    where
        K: Hash + Eq,
    {
        assert!(self.is_creator);
        let hash = self.get_hash(&key);
        let curr_map = self.bucket_vec.load();
        let spot = hash.checked_rem((*curr_map).len())?;
        (*curr_map).get(spot)?.insert(key, value);
        Some(())
    }

    fn insert_raw(&self, to_insert: Arc<(K, AtomicCell<V>)>) -> Option<()>
    where
        K: Hash + Eq,
    {
        assert!(self.is_creator);
        let hash = self.get_hash(&to_insert.0);
        let curr_map = self.bucket_vec.load();
        let spot = hash.checked_rem((*curr_map).len())?;
        (*curr_map).get(spot)?.insert_raw(to_insert);

        Some(())
    }

    pub fn remove(&self, key: &K) -> Option<()>
    where
        K: Hash + Eq,
    {
        assert!(self.is_creator);
        let hash = self.get_hash(key);
        let curr_map = self.bucket_vec.load();
        let spot = hash.checked_rem((*curr_map).len())?;
        (*curr_map).get(spot)?.remove(key);
        Some(())
    }

    pub fn get_iter(&self) -> MapIter<K, V> {
        MapIter {
            snap: self.bucket_vec.load(),
            curr_bucket: None,
            curr_map_index: 0,
            curr_b_index: 0,
        }
    }

    pub fn resize(&self) -> Option<()>
    where
        K: Hash + Eq,
    {
        let mut len = self.bucket_vec.load().len();
        match len {
            0 => len = 1,
            _ => len *= 2,
        }

        let mut new_bucket_vec = Vec::with_capacity(len);
        for _ in 0..len {
            new_bucket_vec.push(Bucket::new());
        }

        for pair in self.get_iter() {
            let spot = self.get_hash(&pair.0).checked_rem(new_bucket_vec.len())?;
            new_bucket_vec.get(spot)?.insert_raw(pair);
        }
        (*self.bucket_vec).store(new_bucket_vec);
        Some(())
    }

    pub fn new_editor(&self) -> Self {
        Self {
            is_creator: false,
            bucket_vec: self.bucket_vec.clone(),
        }
    }
}

pub struct MapIter<K, V> {
    snap: Arc<Vec<Bucket<K, V>>>,
    curr_bucket: Option<Arc<Vec<Arc<(K, AtomicCell<V>)>>>>,
    curr_map_index: usize,
    curr_b_index: usize,
}

impl<K, V> Iterator for MapIter<K, V> {
    type Item = Arc<(K, AtomicCell<V>)>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.curr_bucket.is_none() {
                if let Some(bucket) = self.snap.get(self.curr_map_index) {
                    self.curr_bucket = Some((*bucket).get_full());
                    self.curr_map_index += 1;
                } else {
                    return None;
                }
            }

            if let Some(vector) = &self.curr_bucket {
                if let Some(pair) = vector.get(self.curr_b_index) {
                    self.curr_b_index += 1;
                    return Some(pair.clone());
                } else {
                    self.curr_bucket = None;
                    self.curr_b_index = 0;
                    continue;
                }
            } else {
                panic!("Should be unreachable")
            }
        }
    }
}

struct Bucket<K, V> {
    pair_vec: AtomicCell<Vec<Arc<(K, AtomicCell<V>)>>>,
}

impl<K, V> Bucket<K, V> {
    fn new() -> Self {
        Self {
            pair_vec: AtomicCell::new(Vec::new()),
        }
    }

    fn get_full(&self) -> Arc<Vec<Arc<(K, AtomicCell<V>)>>> {
        self.pair_vec.load()
    }
}

impl<K: Hash + Eq, V> Bucket<K, V> {
    fn get(&self, key: &K) -> Option<Arc<V>> {
        Some((*(self.get_handle(key)?)).1.load())
    }

    fn get_handle(&self, key: &K) -> Option<Arc<(K, AtomicCell<V>)>> {
        for pair in self.pair_vec.load().iter() {
            if pair.0 == *key {
                return Some(pair.clone());
            }
        }
        None
    }

    fn edit(&self, key: &K, value: V) {
        for pair in self.pair_vec.load().iter() {
            if pair.0 == *key {
                pair.1.store(value);
                return;
            }
        }
    }

    fn insert(&self, key: K, value: V) {
        let snap = self.pair_vec.load();

        for pair in snap.iter() {
            if pair.0 == key {
                pair.1.store(value);
                return;
            }
        }

        let mut new = Vec::new();

        for pair in snap.iter() {
            new.push((*pair).clone());
        }

        new.push(Arc::new((key, AtomicCell::new(value))));

        self.pair_vec.store(new);
    }

    fn insert_raw(&self, to_insert: Arc<(K, AtomicCell<V>)>) {
        let snap = self.pair_vec.load();

        let mut new = Vec::new();

        for pair in snap.iter() {
            new.push((*pair).clone());
        }

        new.push(to_insert);

        self.pair_vec.store(new);
    }

    fn remove(&self, key: &K) {
        let snap = self.pair_vec.load();

        let mut new = Vec::new();

        for pair in snap.iter() {
            if pair.0 != *key {
                new.push((*pair).clone());
            }
        }

        self.pair_vec.store(new);
    }
}

impl<K: Hash + Eq, V: Clone> Bucket<K, V> {
    fn get_owned(&self, key: &K) -> Option<V> {
        if let Some(arcy_boi) = self.get(key) {
            return Some((*arcy_boi).clone());
        }
        None
    }
}

pub fn single_test() {
    let map = MlcMap::new_with_capacity(64);
    map.insert("key", "schakalaga");
    dbg! {map.get(&"key")};
    map.remove(&"key");
    dbg!(map.get(&"key"));
    map.insert("key", "value");
    dbg!(map.get(&"key"));
    map.edit(&"key", "value23");
    dbg!(map.get(&"key"));

    let fancycell = AtomicCell::new(8u8);
    println!("Tada: {:?}", fancycell.swap(68u8));
    println!("Voila: {:?}", fancycell.load());
}
