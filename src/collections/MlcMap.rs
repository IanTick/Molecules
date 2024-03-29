// WIP Ignore

/* use crate::primitives::AtomicCell::*;
use crate::collections::MlcVec::*;
#[deny(clippy::pedantic)]
use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
// ctries or concurrent hamt
/*pub struct MlcMap<K, V> {
    bucket_vec: Arc<AtomicCell<Vec<Bucket<K, V>>>>,
}
*/

pub struct AtomicMap<K, V> {
    bucket_line: AtomicVec<Bucket<K, V>>
}

pub struct Bucket<K, V> {
    contents: AtomicVec<Entry<K, V>>
}

pub struct Entry<K, V> {
    key: K,
    value: AtomicCell<V>, // This should be inside an ACell -> Move to wrap the entire entry at bucket level?
}

impl<K, V> AtomicMap<K, V> {
    fn get_hash(&self, key: &K) -> usize
    where
        K: Hash + Eq,
    {
        let mut hasher_instance = DefaultHasher::new();
        key.hash(&mut hasher_instance);
        hasher_instance.finish() as usize
    }

    // TODO: Set Capacity
    fn new() -> Self {
        Self {
            bucket_line: AtomicVec::new(),
        }
    }

    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            bucket_line: AtomicVec::new_with_capacity(capacity),
        }
    }

    pub fn get_handle(&self, key: &K) -> Option<Arc<(K, AtomicCell<V>)>>
    where
        K: Hash + Eq,
    {
        let hash = self.get_hash(key);

        let current_bucket_line = self.bucket_line.get_beam();
        let spot = hash.checked_rem((*current_bucket_line).len())?;

        (*current_bucket_line).get(spot)?.get_handle(key)
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

    pub fn write(&self, key: &K, value: V) -> Option<()>
    where
        K: Hash + Eq,
    {
        (*(self.get_handle(key)?)).1.store(value);
        Some(())
    }

    pub fn insert(&self, key: K, value: V)
    where
        K: Hash + Eq,
    {

        let hash = self.get_hash(&key);
        let mut tries = Vec::new();
        
        // TODO implemennt stage, unstage, commit
        let _output = self.bucket_line.update(|current_bucket_line|
        {
            let spot = std::ops::Rem::rem(hash, (*current_bucket_line).len());
            let insert_here = current_bucket_line[spot].clone();
            
            // Tells the bucket to soon receive a new entry
            // use key_hash, value_hash instead ?
            (*insert_here).stage(&key, &value);
            tries.push(insert_here);
            (current_bucket_line, ())
        }
        )
        // Assume no panic
        .unwrap();


        let success = tries.pop();
        
        success.commit(key, value);

        for fail in tries{
            fail.unstage(&key, &value);
        }

        todo!()
    }

    /* TODO: implement insert_raw
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

    */

    pub fn remove(&self, key: &K) -> Option<V>
    where
        K: Hash + Eq,
    {
        use std::ops::Rem;
        let hash = self.get_hash(key);
        let current_bucket_line = self.bucket_line.get_beam();
        let spot = hash.rem(current_bucket_line.len());
        // TODO make bucket.remove return the value
        let v = (*current_bucket_line).get(spot)?.remove(key);
        Some(v)
    }

    pub fn get_iter(&self) -> MapIter<K, V> {
        MapIter {
            snap: self.bucket_line.get_beam(),
            curr_bucket: None,
            curr_map_index: 0,
            curr_b_index: 0,
        }
    }


    // TODO continue here
    // Well fuck.

    // Can resize no problemo through a shared ref, but but 1/2 Bucket_entries may be at the wrong place.
    

    // Possible? But bad.
    // Practical resize with a warn flag? => exclusive resize access.
    // Fetch_update the bucket_line with double length.
    // Set double true
    // Iterate over the old buckets and move wrongly placed entries.
    // (First copy the entry, then remove it.)
    // Once finished turn off warn flag.
    // During warn flag:
    // reads ask a .get on double the calculated idx.
    // Inserts check len, insert accordingly and check len again, if different undo and retry
    // removes see if it would move, if not, they remove, otherwise they must be postponed.
    // Turn off warn flag => do postponed removes

    // Following is true for all resizes => resizes must lock down the data structure, therefore require &mut, actually arcs to buckets may be held? => Must not expose internals => arc to the internals (other than a value, which is fine) require a holding a ref to the map aswewll => &mut means no arcs to internals
    // If an insert sleeps between identifying a bucket and inserting the value, and a resize comes in and completes, then the insert happens 50% (for each complete resize) of the time at the wrong place 
    pub fn resize(&mut self) -> Option<()>
    where
        K: Hash + Eq,
    {
        let mut current_bucket_line = self.bucket_line.get_beam();
        
        let new_length = match current_bucket_line.len() {
            0 => 1,
            l => l * 2,
        };

        let mut new_bucket_line = AtomicVec::new_with_capacity(new_length);
        
        for _ in 0..new_length {
            new_bucket_line.push(Bucket::new());
        }


        for pair in self.get_iter() {
            let spot = self.get_hash(&pair.0).checked_rem(new_length)?;
            // This doesnt work
            new_bucket_line.get(spot)?.stage(&pair.0, &pair.1);
        };

        // Something, something, cas
        (*self.bucket_line).store(new_bucket_vec);
        Some(())
    }
}

pub struct MapIter<K, V> {
    snap: Arc<Vec<Arc<Bucket<K, V>>>>,
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
*/