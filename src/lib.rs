mod acell {
    #[deny(clippy::pedantic)]
    use std::sync::atomic::{compiler_fence, AtomicBool, AtomicPtr, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        marker::PhantomData,
    };

    pub struct AtomicCell<T> {
        load_counter: AtomicUsize,
        ptr: AtomicPtr<ACNode<T>>,
        _marker: PhantomData<ACNode<T>>,
    }

    impl<T> AtomicCell<T> {
        pub fn new(value: T) -> Self {
            let cell = Self {
                load_counter: AtomicUsize::new(0),
                ptr: AtomicPtr::new(ACNode::new(value)),
                _marker: PhantomData,
            };
            unsafe {
                (*(cell.ptr.load(Ordering::Acquire)))
                    .chained_flag
                    .store(true, Ordering::Release);
            }
            cell
        }

        pub fn store(&self, value: T) {
            let to_acnode = ACNode::new(value);

            let old = self.ptr.swap(to_acnode, Ordering::AcqRel);

            unsafe {
                (*to_acnode).next = old;
                compiler_fence(Ordering::Release);
                (*to_acnode).chained_flag.store(true, Ordering::Release);
            }

            if self.load_counter.load(Ordering::Acquire) == 0 {
                unsafe {
                    self.free(to_acnode);
                }
            }
        }

        pub fn load(&self) -> Arc<T> {
            self.load_counter.fetch_add(1, Ordering::AcqRel); // Release should work just fine.

            let latest = self.ptr.load(Ordering::Acquire);
            let ret_val = unsafe { (*latest).value.clone() };

            if self.load_counter.fetch_sub(1, Ordering::AcqRel) == 1 {
                // Free some memory, starting at "latest"
                unsafe {
                    self.free(latest);
                }
            }
            ret_val
        }

        pub fn swap(&self, value: T) -> Arc<T> {
            let to_acnode = ACNode::new(value);

            let old = self.ptr.swap(to_acnode, Ordering::AcqRel);

            let ret_val;
            unsafe {
                ret_val = (*old).value.clone();

                (*to_acnode).next = old;
                compiler_fence(Ordering::Release);
                (*to_acnode).chained_flag.store(true, Ordering::Release);
            }

            if self.load_counter.load(Ordering::Acquire) == 0 {
                unsafe {
                    self.free(to_acnode);
                };
            }

            ret_val
        }

        unsafe fn free(&self, latest: *mut ACNode<T>) {
            match (*latest).chained_flag.compare_exchange(
                true,
                false,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let mut next_next_ptr = (*latest).next;

                    if !(next_next_ptr == latest)
                    // First node is not self-ref.
                    {
                        let mut prev_next_ptr = next_next_ptr.clone();

                        loop {
                            // prev_next_ptr is the read ptr from the previous iteration;
                            match (*prev_next_ptr).chained_flag.compare_exchange(
                                true,
                                false,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            ) {
                                // Read the next ptr of the node
                                // Note: We never deref next_next_ptr! Only as prev_next_ptr in the following iteration!
                                Ok(_) => {
                                    next_next_ptr = (*prev_next_ptr).next;

                                    if next_next_ptr == prev_next_ptr {
                                        // This node is self-referential. Drop it! As it was the last node, we are done.
                                        let drop_this = Box::from_raw(prev_next_ptr);
                                        drop(drop_this); // gonna be explicit here :)
                                                         // Make the first node self-ref, to mark as end.
                                        (*latest).next = latest;
                                        compiler_fence(Ordering::Release);
                                        (*latest).chained_flag.store(true, Ordering::Release);
                                        break;
                                    } else {
                                        // This node has a next. Drop this node and proceed with its next ptr.
                                        let drop_this = Box::from_raw(prev_next_ptr);
                                        drop(drop_this); // gonna be explicit here :)
                                        prev_next_ptr = next_next_ptr;
                                    }
                                }
                                Err(_) => {
                                    // This node is not chained, we cant drop it and we cant proceed. Therefore we "bridge" to it for future frees. Then we are done.
                                    (*latest).next = prev_next_ptr;
                                    compiler_fence(Ordering::Release);
                                    (*latest).chained_flag.store(true, Ordering::Release);
                                    break;
                                }
                            }
                            /*
                            Follow the previous next ptr.
                            Check if the new ACNode is chained. If not, end.
                            Check if the new ACNode is self-referential. If so, then its final, dealloc it and then end.
                            Otherwise, if its neiter the final nor (the first, checked outside of loop nor) not init, dealloc it. And repeat again using its next-ptr
                            in the next iteration.
                            */
                        }
                    }
                    else {
                        (*latest).chained_flag.store(true, Ordering::Release);
                    }
                }

                Err(_) => (),
            }
        }
    }

    impl<T> Drop for AtomicCell<T> {
        fn drop(&mut self) {
            // No reference to AtomicCell exists, since its dropping.
            self.load(); // Drops all but the current ACNode
            println!("{:?}", self.load_counter.load(Ordering::Acquire));
            let latest = self.ptr.load(Ordering::Acquire);
            unsafe {
                let boxed_last_node = Box::from_raw(latest);
                drop(boxed_last_node);
            }
        }
    }

    unsafe impl<T: Send> Send for AtomicCell<T> {}
    unsafe impl<T: Send + Sync> Sync for AtomicCell<T> {}

    struct ACNode<T> {
        next: *mut Self,
        value: Arc<T>,
        chained_flag: AtomicBool,
    }

    impl<T> ACNode<T> {
        // Init can be assumed of next.
        fn new(value: T) -> *mut Self {
            let false_ptr: *mut Self = std::ptr::null_mut();

            let pre = Self {
                next: false_ptr,
                value: Arc::new(value),
                chained_flag: AtomicBool::new(false),
            };

            let boxed = Box::new(pre);
            let correct_ptr = Box::into_raw(boxed);

            unsafe {
                (*correct_ptr).next = correct_ptr; // next is now ptr to self on heap. Self is "leaked".
            }
            correct_ptr
        }
    }

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
}

#[cfg(test)]
mod tests {
    use crate::acell::*;
    use std::{sync::Arc, thread};

    fn store_bash<T>(cell: Arc<AtomicCell<T>>, new: T)
    where
        T: Clone,
    {
        for _ in 0..10 {
            cell.store(new.clone())
        }
    }

    fn load_bash<T>(cell: Arc<AtomicCell<T>>) {
        for _ in 0..10 {
            cell.load();
        }
    }

    fn swap_bash<T>(cell: Arc<AtomicCell<T>>, new: T)
    where
        T: Clone,
    {
        for _ in 0..10 {
            cell.swap(new.clone());
        }
    }

    #[test]
    fn it_works() {
        single_test();
    }

    #[test]
    fn acell_store() {
        let fancy_cell = Arc::new(AtomicCell::new("Bonjour"));
        let arcyboi1 = fancy_cell.clone();
        let arcyboi2 = fancy_cell.clone();
        let arcyboi3 = fancy_cell.clone();
        let w1 = thread::spawn(move || store_bash(arcyboi1, "Adieu!"));
        let w2 = thread::spawn(move || store_bash(arcyboi2, "Adieu Amigo!"));
        let w3 = thread::spawn(move || store_bash(arcyboi3, "Au Revoir!"));

        w1.join();
        w2.join();
        w3.join();

        let val = fancy_cell.load();

        assert!({ *val == "Adieu" || *val == "Adieu Amigo!" || *val == "Au Revoir!" })
    }

    #[test]
    fn acell_load_store() {
        let fancy_cell = Arc::new(AtomicCell::new("Bonjour"));
        let arcyboi1 = fancy_cell.clone();
        let arcyboi2 = fancy_cell.clone();
        let arcyboi3 = fancy_cell.clone();
        let arcyboi4 = fancy_cell.clone();
        let arcyboi5 = fancy_cell.clone();
        let arcyboi6 = fancy_cell.clone();
        let w1 = thread::spawn(move || store_bash(arcyboi1, "Adieu!"));
        let w2 = thread::spawn(move || store_bash(arcyboi2, "Adieu Amigo!"));
        let w3 = thread::spawn(move || store_bash(arcyboi3, "Au Revoir!"));
        let w4 = thread::spawn(move || load_bash(arcyboi4));
        let w5 = thread::spawn(move || load_bash(arcyboi5));
        let w6 = thread::spawn(move || load_bash(arcyboi6));

        w1.join();
        w2.join();
        w3.join();
        w4.join();
        w5.join();
        w6.join();

        let val = fancy_cell.load();

        assert!({ *val == "Adieu" || *val == "Adieu Amigo!" || *val == "Au Revoir!" })
    }

    #[test]
    fn acell_all() {
        let fancy_cell = Arc::new(AtomicCell::new("Bonjour"));
        let arcyboi1 = fancy_cell.clone();
        let arcyboi2 = fancy_cell.clone();
        let arcyboi3 = fancy_cell.clone();
        let arcyboi4 = fancy_cell.clone();
        let arcyboi5 = fancy_cell.clone();
        let arcyboi6 = fancy_cell.clone();
        let arcyboi7 = fancy_cell.clone();
        let arcyboi8 = fancy_cell.clone();
        let arcyboi9 = fancy_cell.clone();
        let w1 = thread::spawn(move || store_bash(arcyboi1, "Adieu!"));
        let w2 = thread::spawn(move || store_bash(arcyboi2, "Adieu Amigo!"));
        let w3 = thread::spawn(move || store_bash(arcyboi3, "Au Revoir!"));
        let w4 = thread::spawn(move || load_bash(arcyboi4));
        let w5 = thread::spawn(move || load_bash(arcyboi5));
        let w6 = thread::spawn(move || load_bash(arcyboi6));
        let w7 = thread::spawn(move || swap_bash(arcyboi7, "a"));
        let w8 = thread::spawn(move || swap_bash(arcyboi8, "b"));
        let w9 = thread::spawn(move || swap_bash(arcyboi9, "c"));
        
        assert!(w1.join().is_ok());
        assert!(w2.join().is_ok());
        assert!(w3.join().is_ok());
        assert!(w4.join().is_ok());
        assert!(w5.join().is_ok());
        assert!(w6.join().is_ok());
        assert!(w7.join().is_ok());
        assert!(w8.join().is_ok());
        assert!(w9.join().is_ok());



        let val = fancy_cell.load();

        //assert!({ *val == "Adieu" || *val == "Adieu Amigo!" || *val == "Au Revoir!" });
        println!("booyah");
        }
}
