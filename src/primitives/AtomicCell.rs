use std::marker::PhantomData;
#[deny(clippy::pedantic)]
use std::sync::atomic::{compiler_fence, AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

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
                } else {
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