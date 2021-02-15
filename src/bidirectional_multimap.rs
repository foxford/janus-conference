use std::borrow::Borrow;
use std::fmt::Debug;
use std::hash::Hash;

use multimap::{IterAll, MultiMap};

#[derive(Debug)]
pub struct BidirectionalMultimap<K: Eq + Hash, V: Eq + Hash> {
    forward_mapping: MultiMap<K, V>,
    inverse_mapping: MultiMap<V, K>,
}

impl<K, V> BidirectionalMultimap<K, V>
where
    K: Eq + Hash + Clone + Debug,
    V: Eq + Hash + Clone + Debug,
{
    pub fn new() -> Self {
        Self {
            forward_mapping: MultiMap::new(),
            inverse_mapping: MultiMap::new(),
        }
    }

    pub fn associate(&mut self, k: K, v: V) {
        let kk = k.clone();
        let vv = v.clone();
        self.forward_mapping.insert(k, vv);
        self.inverse_mapping.insert(v, kk);
    }

    pub fn remove_key<T>(&mut self, k: &T) -> Option<Vec<V>>
    where
        K: Borrow<T>,
        T: Hash + Eq + Debug,
    {
        let vs = self.forward_mapping.remove(k);

        if let Some(ref vs) = vs {
            for v in vs {
                if let Some(ks) = self.inverse_mapping.get_vec_mut(&v) {
                    ks.retain(|x| x.borrow() != k);
                } else {
                    err!(
                        "Map in inconsistent state: entry ({:?}, {:?}) has no corresponding entry.",
                        k,
                        v
                    );
                }
            }
        }

        vs
    }

    pub fn remove_value<U>(&mut self, v: &U)
    where
        V: Borrow<U>,
        U: Hash + Eq + Debug,
    {
        if let Some(keys) = self.inverse_mapping.remove(v) {
            for k in keys {
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
    }

    pub fn remove_pair<T, U>(&mut self, k: &T, v: &U)
    where
        K: Borrow<T>,
        T: Hash + Eq + Debug,
        V: Borrow<U>,
        U: Hash + Eq + Debug,
    {
        if let Some(vals) = self.forward_mapping.get_vec_mut(k) {
            vals.retain(|x| x.borrow() != v);
        }

        if let Some(vals) = self.inverse_mapping.get_vec_mut(&v) {
            vals.retain(|x| x.borrow() != k);
        } else {
            err!(
                "Map in inconsistent state: entry ({:?}, {:?}) has no corresponding entry.",
                k,
                v
            );
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

    pub fn get_keys<U>(&self, v: &U) -> &[K]
    where
        V: Borrow<U>,
        U: Hash + Eq,
    {
        self.inverse_mapping
            .get_vec(v)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn iter_all(&self) -> IterAll<'_, K, Vec<V>> {
        self.forward_mapping.iter_all()
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    fn build_bmm() -> BidirectionalMultimap<usize, usize> {
        let mut bmm = BidirectionalMultimap::new();
        bmm.associate(11, 1);
        bmm.associate(12, 1);
        bmm.associate(21, 2);
        bmm.associate(22, 2);
        bmm
    }

    #[test]
    fn associate() {
        let mut bmm = build_bmm();
        bmm.associate(11, 3);

        assert_eq!(bmm.get_values(&11), &[1, 3]);
        assert_eq!(bmm.get_values(&12), &[1]);
        assert_eq!(bmm.get_values(&21), &[2]);
        assert_eq!(bmm.get_values(&22), &[2]);
        assert_eq!(bmm.get_keys(&1), &[11, 12]);
        assert_eq!(bmm.get_keys(&2), &[21, 22]);
        assert_eq!(bmm.get_keys(&3), &[11]);
        assert!(bmm.get_keys(&4).is_empty());
    }

    #[test]
    fn remove_key() {
        let mut bmm = build_bmm();
        bmm.associate(21, 1);

        assert_eq!(bmm.remove_key(&21), Some(vec![2, 1]));
        assert_eq!(bmm.get_values(&11), &[1]);
        assert_eq!(bmm.get_values(&12), &[1]);
        assert!(bmm.get_values(&21).is_empty());
        assert_eq!(bmm.get_values(&22), &[2]);
        assert_eq!(bmm.get_keys(&1), &[11, 12]);
        assert_eq!(bmm.get_keys(&2), &[22]);
    }

    #[test]
    fn remove_key_missing() {
        let mut bmm = build_bmm();

        assert_eq!(bmm.remove_key(&3), None);
        assert_eq!(bmm.get_values(&11), &[1]);
        assert_eq!(bmm.get_values(&12), &[1]);
        assert_eq!(bmm.get_values(&21), &[2]);
        assert_eq!(bmm.get_values(&22), &[2]);
        assert_eq!(bmm.get_keys(&1), &[11, 12]);
        assert_eq!(bmm.get_keys(&2), &[21, 22]);
    }

    #[test]
    fn remove_value() {
        let mut bmm = build_bmm();
        bmm.remove_value(&2);

        assert_eq!(bmm.get_values(&11), &[1]);
        assert_eq!(bmm.get_values(&12), &[1]);
        assert!(bmm.get_values(&21).is_empty());
        assert!(bmm.get_values(&22).is_empty());
        assert_eq!(bmm.get_keys(&1), &[11, 12]);
        assert!(bmm.get_keys(&2).is_empty());
    }

    #[test]
    fn remove_value_missing() {
        let mut bmm = build_bmm();
        bmm.remove_value(&3);

        assert_eq!(bmm.get_values(&11), &[1]);
        assert_eq!(bmm.get_values(&12), &[1]);
        assert_eq!(bmm.get_values(&21), &[2]);
        assert_eq!(bmm.get_values(&22), &[2]);
        assert_eq!(bmm.get_keys(&1), &[11, 12]);
        assert_eq!(bmm.get_keys(&2), &[21, 22]);
    }

    #[test]
    fn remove_pair() {
        let mut bmm = build_bmm();
        bmm.associate(21, 1);
        bmm.remove_pair(&21, &2);

        assert_eq!(bmm.get_values(&11), &[1]);
        assert_eq!(bmm.get_values(&12), &[1]);
        assert_eq!(bmm.get_values(&21), &[1]);
        assert_eq!(bmm.get_values(&22), &[2]);
        assert_eq!(bmm.get_keys(&1), &[11, 12, 21]);
        assert_eq!(bmm.get_keys(&2), &[22]);
    }

    #[test]
    fn iter_all() {
        let mut bmm = build_bmm();
        bmm.associate(21, 1);
        let entries = bmm.iter_all().collect::<Vec<(&usize, &Vec<usize>)>>();

        let expected_entries = vec![
            (11, vec![1]),
            (12, vec![1]),
            (21, vec![2, 1]),
            (22, vec![2]),
        ];

        for (k, vs) in expected_entries {
            assert!(entries.contains(&(&k, &vs)));
        }
    }
}
