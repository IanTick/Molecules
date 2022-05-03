# Molecules

This repository experiments with concurrent data structures and primitives (ideally wait-free).
Use at your own risk :)


# AtomicCell

The main primitive of this repo, found in src/primitives/AtomicCell.
AtomicCell<T> is an opaque wrapper and allows basic atomic operations on any type T.
These operations are:
	
- load  -> Returns the current value. (As an Arc).
- store -> Stores a new value inside the AtomicCell.	
- swap  -> Stores a new value inside the AtomicCell and returns the previous value which it replaced (As an Arc).


At its core, AtomicCell<T> uses an AtomicPointer to switch to new values of type T. However old values must still be safely dropped. To achieve this each call to store and swap swaps the AtomicPointer out, thereby preserving information of where old values are in memory. The pointer to a preceding value is stored alongside the current value, forming a (potentially segmentated) linked list. The private free function can walk this segmentated linked list "downstream" and free memory.
	
Currently no known errors/bugs exist for AtomicCell. Please help verify the soundness of AtomicCell by looking into the code and/or testing it.
	
	
# MlcMap
	
A first draft of a concurrent hashmap using AtomicCell.
	
MlcMap allows one "creator" to make structural changes to the map, such as inserting, removing and resizing.
Any number of "editors" can make changes to values associated by keys.
