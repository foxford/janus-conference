use std::borrow::Borrow;
use std::fmt::Debug;
use std::hash::Hash;

use fnv::FnvHashMap;
use multimap::MultiMap;

#[derive(Debug)]
pub struct BidirectionalMultimap<K: Eq + Hash, V: Eq + Hash> {
    forward_mapping: MultiMap<K, V>,
    inverse_mapping: FnvHashMap<V, K>,
}

#[allow(dead_code)]
impl<K, V> BidirectionalMultimap<K, V>
where
    K: Eq + Hash + Clone + Debug,
    V: Eq + Hash + Clone + Debug,
{
    pub fn new() -> Self {
        Self {
            forward_mapping: MultiMap::new(),
            inverse_mapping: FnvHashMap::default(),
        }
    }

    pub fn keys_count(&self) -> usize {
        self.forward_mapping.len()
    }

    pub fn associate(&mut self, k: K, v: V) {
        let kk = k.clone();
        let vv = v.clone();
        self.forward_mapping.insert(k, vv);
        self.inverse_mapping.insert(v, kk);
    }

    pub fn disassociate<T, U>(&mut self, k: &T, v: &U)
    where
        K: Borrow<T>,
        T: Hash + Eq,
        V: Borrow<U>,
        U: Hash + Eq,
    {
        if let Some(vals) = self.forward_mapping.get_vec_mut(k) {
            vals.retain(|x| x.borrow() != v);
        }
        self.inverse_mapping.remove(v);
    }

    pub fn remove_key<T>(&mut self, k: &T) -> Option<Vec<V>>
    where
        K: Borrow<T>,
        T: Hash + Eq + Debug,
    {
        let vs = self.forward_mapping.remove(k);

        if let Some(ref vs) = vs {
            for v in vs {
                self.inverse_mapping.remove(v);
            }
        }

        vs
    }

    pub fn remove_value<U>(&mut self, v: &U)
    where
        V: Borrow<U>,
        U: Hash + Eq + Debug,
    {
        if let Some(k) = self.inverse_mapping.remove(v) {
            if let Some(vs) = self.forward_mapping.get_vec_mut(&k) {
                vs.retain(|x| x.borrow() != v);
            } else {
                err!(
                    "Map in inconsistent state: entry ({:?}, {:?}) has no corresponding entry.",
                    k,
                    v
                );
            }
        }
    }

    pub fn get_values<T>(&self, k: &T) -> &[V]
    where
        K: Borrow<T>,
        T: Hash + Eq,
    {
        self.forward_mapping
            .get_vec(k)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn contains_key<T>(&self, k: &T) -> bool
    where
        K: Borrow<T>,
        T: Hash + Eq,
    {
        self.forward_mapping.contains_key(k)
    }

    pub fn get_key<U>(&self, v: &U) -> Option<&K>
    where
        V: Borrow<U>,
        U: Hash + Eq,
    {
        self.inverse_mapping.get(v)
    }
}
