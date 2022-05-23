#[deny(clippy::pedantic)]
use std::marker::PhantomData;
use std::sync::atomic::{compiler_fence, AtomicBool, AtomicPtr, AtomicUsize, Ordering, fence};
use std::sync::Arc;

/* AtomicCell<T> simulates basic atomic operations on any type T. It mimics the behaviour of actual atomics:

                |              |                |
AtomicU64       | .store(u64)  | .load()        | .swap(u64)
                |  u64 -> ()   |    -> u64      |   u64 -> u64          *Memory Ordering omitted.
--------------------------------------------------------------------
                |              |                |
AtomicCell<u64> | .store(u64)  | .load()        | .swap(u64)
                |  u64 -> ()   |    -> Arc<u64> |   u64 -> Arc<u64>     *Memory Ordering is always Acq/Rel.


Note that T may or may not be 'Copy'. To avoid extra allocation only an Arc<T> is returned. If T is 'Clone' an owned
return value is trivial. */

pub struct AtomicCell<T> {
    /* How many loads are currently in progress. After a load operation is finished it can decrement this value again.
    Swaps do not load. */
    load_counter: AtomicUsize,
    /* An 'AtomicPtr' to the latest stored value of T. The 'ACNode<T>' contains the value and other important information for freeing memory.*/
    ptr: AtomicPtr<ACNode<T>>,
    /* When 'AtomicCell<T>' is dropped then so is 'ACNode<T>' and hence some T. This has to be known by the compiler as
    'AtomicCell<T>' does - itself - not "hold" an instance of T */
    _marker: PhantomData<ACNode<T>>,
}

/* No assumptions about T is made. (As of right now it still need to be 'Sized') */
impl<T> AtomicCell<T> {
    /* Simply constructs a new 'AtomicCell<T>', it obviously takes ownership of values. From creation to destruction there must ALWAYS
    be a valid T stored inside the AtomicCell. */
    pub fn new(value: T) -> Self {
        let cell = Self {
            load_counter: AtomicUsize::new(0),
            /* ACNode::new() returns a pointer from a Box::into_raw() */
            ptr: AtomicPtr::new(ACNode::new(value)),
            _marker: PhantomData,
        };

        /* The ACNode contains a "chained flag" which marks whether a given ACNode is "chained" to its preceeding ACNodes.
        As this is the first ACNode created it should be chained.*/
        unsafe {
            (*(cell.ptr.load(Ordering::Acquire)))
                .chained_flag
                .store(true, Ordering::Release);
        }
        cell
    }

    /* Takes a value of type T and stores it into the AtomicCell. Any loads and/or swaps that happen after a store will see only the latest value stored. */
    pub fn store(&self, value: T) {
        let to_acnode = ACNode::new(value);

        /* The AtomicPtr makes this operation atomic. Any future accesses now follow the new pointer to the new ACNode.
        However some bookkeeping has to be done with the old ACNode. */
        let old = self.ptr.swap(to_acnode, Ordering::AcqRel);

        /* This links the ACNode we just made to the old ACNode. Afterwards the new ACNode is considered "chained" because it points to it predecessor. */
        unsafe {
            (*to_acnode).next = old;
            compiler_fence(Ordering::Release);
            (*to_acnode).chained_flag.store(true, Ordering::Release);
        }

        /* Lastly it checks if freeing of memory can be done. */
        if self.load_counter.load(Ordering::Acquire) == 0 {
            unsafe {
                self.free(to_acnode);
            }
        }
    }

    /* Loading is a very simple task. It simply follows the 'AtomicPtr' and reads the value stored in the current ACNode. Loads will always only get the latest value. */
    pub fn load(&self) -> Arc<T> {
        /* Marks that we perform a load operation. Since a thread may be stuck between getting the ptr and derefing it no frees must happen while a load is in progress.
        Otherwise the ACNode may be removed from under our feet. */
        self.load_counter.fetch_add(1, Ordering::AcqRel);
        let latest = self.ptr.load(Ordering::Acquire);
        /* .value of the ACNode stores an Arc */
        let ret_val = unsafe { (*latest).value.clone() };

        /* Again, free memory if possible. And mark the load operation as completed. */
        if self.load_counter.fetch_sub(1, Ordering::AcqRel) == 1 {
            unsafe {
                self.free(latest);
            }
        }
        ret_val
    }

    /* Swap resembles a store operation. In addition if also follows the "old-pointer" to its predecessor to get its value.
    Swaps always return the value they replaced. Swaps do not load.*/
    pub fn swap(&self, value: T) -> Arc<T> {
        let to_acnode = ACNode::new(value);

        let old = self.ptr.swap(to_acnode, Ordering::AcqRel);

        let ret_val;
        unsafe {
            ret_val = (*old).value.clone(); // Simply gets the old ACNode's value.

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

    /* This function performs heavy logic to free memory. It is best understood after reading the implementation of ACNode.
    It is marked as unsafe since it uses a raw pointer argument and requires that no threads hold pointers to the given ACNodes predecessors!
    -> Guaranteed by load_counter.
    Not public! */
    unsafe fn free(&self, latest: *mut ACNode<T>) {
        /* Remember the "chained flag" of ACNode? It signals whether an ACNode is fully initialized. To perform any operation we "unchain" the ACNode
        thereby guranteering that is was chained and that no other thread can operate on it. */

        match (*latest).chained_flag.compare_exchange(
            true,
            false,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            /* If the cas on the first ACNode succeeds we can proceed...
            Remember: "latest" is the pointer to the very first ("latest") ACNode. */
            Ok(_) => {
                /* Think of the following code as walking down the nodes of a linked list. There are 3 pointers involved:
                - latest -> the very first pointer (head).
                - prev_next_ptr -> the pointer with which we arrived at the ACNode we are currently at.
                - next_next_ptr -> the pointer from the ACNode are at to another ACNode. This pointer will later replace prev_next_pointer and so on... */
                let mut next_next_ptr = (*latest).next;

                /* Checks if the latest ACNode is self-referential. Self-reference marks some "end" in the list.*/
                if !(next_next_ptr == latest)
                // First node is not self-ref.
                {
                    /* Now we go one ACNode deep

                                         |
                                         | (latest)
                                         |
                        ---------    ----------
                        -       <-----        -
                        --------- |  ----------
                              prev_next_ptr ( old next_next_ptr)
                    */
                    let mut prev_next_ptr = next_next_ptr.clone();

                    /* Now entering a loop. Note that this loop is finite. (We always make progress, no extra iterations can be created.) */
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
                } else {
                    // Is self-ref, hit undo.
                    (*latest).chained_flag.store(true, Ordering::Release);
                }
            }

            /* If the cas failed than some other thread is working on it, probably freeing the memory for us. Great! We are done. */
            Err(_) => (),
        }
    }

    pub(crate) unsafe fn phantom_double_load(&self) -> (Arc<T>, *mut ACNode<T>) {
        let latest = self.ptr.load(Ordering::Acquire);
        /* .value of the ACNode stores an Arc */
        let ret_val = unsafe { (*latest).value.clone() };
        (ret_val, latest)
    }

    /// UNTESTED!
    pub(crate) unsafe fn cas(
        &self,
        expected: *mut ACNode<T>,
        new: *mut ACNode<T>,
    ) -> Result<(), ()> {
        // let new_node = ACNode::new(new);

        match self
            .ptr
            .compare_exchange(expected, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    /// UNTESTED!
    pub fn fetch_update<O, F>(&self, func: F) -> Option<O>
    where
        F: Fn(Arc<T>) -> (T, Option<O>),
    {
        loop {
            self.load_counter.fetch_add(1, Ordering::AcqRel);
            let (arg, ptr) = unsafe { self.phantom_double_load() };

            let (updated, output) = func(arg);
            let to_new = ACNode::new(updated);

            unsafe {
                match self.cas(ptr, to_new) {
                    Ok(_) => {
                        (*to_new).next = ptr;
                        fence(Ordering::AcqRel);
                        (*to_new).chained_flag.store(true, Ordering::Release);
                        self.load_counter.fetch_sub(1, Ordering::AcqRel);
                        return output;
                    }
                    Err(_) => {
                        self.load_counter.fetch_sub(1, Ordering::AcqRel);
                        drop(Box::from_raw(to_new));
                        continue;
                    }
                }
            }
        }
    }
}

impl<T: Eq> AtomicCell<T> {
    pub fn cas_by_eq(&self, expected: &T, new: T) -> Result<(), ()> {
        let to_new = ACNode::new(new);

        self.load_counter.fetch_add(1, Ordering::AcqRel);
        let latest = self.ptr.load(Ordering::Acquire);

        unsafe {
            if *(*latest).value == *expected {
                match self.cas(latest, to_new) {
                    Ok(_) => {
                        (*to_new).next = latest;
                        fence(Ordering::AcqRel);
                        (*to_new).chained_flag.store(true, Ordering::Release);
                        self.load_counter.fetch_sub(1, Ordering::AcqRel);
                        return Ok(());
                    }
                    Err(_) => (),
                }
            };
            self.load_counter.fetch_sub(1, Ordering::AcqRel);
            drop(Box::from_raw(to_new));
        }
        Err(())
    }
}

impl<T> Drop for AtomicCell<T> {
    fn drop(&mut self) {
        // No reference to AtomicCell exists, since its dropping.
        self.load(); // Drops all but the current ACNode (Load counter = 0)
        let latest = self.ptr.load(Ordering::Acquire);
        unsafe {
            // Manually drop the latest node.
            let boxed_last_node = Box::from_raw(latest);
            drop(boxed_last_node);
        }
    }
}

unsafe impl<T: Send> Send for AtomicCell<T> {}
unsafe impl<T: Send + Sync> Sync for AtomicCell<T> {}

pub(crate) struct ACNode<T> {
    next: *mut Self,
    value: Arc<T>,
    chained_flag: AtomicBool,
}

impl<T> ACNode<T> {
    fn new(value: T) -> *mut Self {
        let false_ptr: *mut Self = std::ptr::null_mut(); // Avoids MaybeUninit

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

    fn into_inner(ptr: *mut Self) -> Result<T, Arc<T>> {
        unsafe {
            let mut boxed = Box::from_raw(ptr);
            let arcyboi = boxed.as_ref().value.clone();
            drop(boxed);
            return Arc::try_unwrap(arcyboi);
        }
    }
}
