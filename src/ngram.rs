use std::iter::{FromIterator, IntoIterator};
use std::hash::{Hash, Hasher};

/// Struct representing a hashable vector where the final hash
/// is the sum of its elements hashes.
#[derive(Eq, PartialEq, Debug, RustcDecodable, RustcEncodable)]
pub struct HashableVec<T: Clone + Eq + Hash> {
    pub obj: Vec<T>,
}

impl<T: Clone + Eq + Hash> HashableVec<T> {
    pub fn new<I: IntoIterator<Item = T>>(iter: I) -> HashableVec<T> {
        HashableVec { obj: Vec::from_iter(iter) }
    }
}

impl<T: Clone + Eq + Hash> Hash for HashableVec<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for item in &self.obj {
            item.hash(state)
        }
    }
}

/// Holds `NGram` of type `T` and size `size`
// TODO: hash^eq
#[derive(Hash, Eq, PartialEq, Debug, RustcDecodable, RustcEncodable)]
pub struct NGram<T: Clone + Eq + Hash> {
    pub size: usize,
    pub elements: HashableVec<T>,
}

impl<T: Clone + Eq + Hash> NGram<T> {
    pub fn new(items: &[T]) -> NGram<T> {
        NGram {
            size: items.len(),
            elements: HashableVec::new(items.iter().cloned()),
        }
    }
}
