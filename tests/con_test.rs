use std::{sync::Arc, thread};
use molecules::collections::MlcMap::*;
use molecules::primitives::AtomicCell::*;

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
    single_test(); // Ignore this.
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
    // Actual test. Kinda slow (with miri). ~20 000 loads/stores.
    let fancy_cell = Arc::new(AtomicCell::new("Bonjour"));
    let arcyboi = fancy_cell.clone();
    let mut vector = Vec::new();

    for _ in 0..1000 {
        let copy = arcyboi.clone();
        let other_copy = fancy_cell.clone();

        let handle = thread::spawn(move || store_bash(other_copy, "Something"));

        vector.push(handle);

        let copy = arcyboi.clone();
        let other_copy = fancy_cell.clone();

        let scnd_handle = thread::spawn(move || load_bash(other_copy));

        vector.push(scnd_handle);
    }

    for i in vector {
        i.join();
    }
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
