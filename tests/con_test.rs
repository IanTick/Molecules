use mlc::primitives::AtomicCell::*;
use std::{sync::Arc, thread};

fn main(){
    println!("Hello from con_test main!");
}


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
        let _ = cell.load();
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
fn first(){
        let x = AtomicCell::new("Bonjour");
        x.store("Bonjour");
        x.store("Bonjour");
    }

// #[test]
fn acell_store() {
    let fancy_cell = Arc::new(AtomicCell::new("Bonjour"));

    (0..10).map(|_|{ let cell = fancy_cell.clone(); thread::spawn(move||{for _ in 0..10{cell.store("Bonjour")}})}).collect::<Vec<_>>().into_iter().for_each(|h|{h.join();});
}


// #[test]
fn summing() {
    let bar = Arc::new(std::sync::Barrier::new(1000));
    let fancy_cell = Arc::new(AtomicCell::new(0u64));

    let vector = (1..=1000u64)
        .map(|_num| {
            let x = fancy_cell.clone();
            let xbar = bar.clone();
            thread::spawn(move || {xbar.wait(); x.fetch_update::<(), _>(|cell| (Arc::new((*cell) + 1), ()))})
        })
        .collect::<Vec<_>>();
    for x in vector{
        _=x.join();
    }

    assert_eq!((*fancy_cell.load()), 1000)
}
