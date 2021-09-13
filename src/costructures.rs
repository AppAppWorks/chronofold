use std::collections::BTreeMap;
use std::mem;

use crate::{IndexShift, LocalIndex, RelativeNextIndex, RelativeReference, Author};
use crate::offsetmap::Offset;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;

macro_rules! costructures_get_btree_range {
    ($cs:expr, $key:tt, $flag:expr, $shift:expr) => {
        {
            let key = $key.0 | $flag << $shift;
            $cs.map.range(($flag << $shift)..=key).map(|(_, v)| v).next_back().cloned()
        }
    }
}

macro_rules! costructures_get_btree_exact {
    ($cs:expr, $key:tt, $flag:expr, $shift:expr) => {
        {
            let key = $key.0 | $flag << $shift;
            $cs.map.get(&key).cloned()
        }
    }
}

macro_rules! costructures_set_btree_range {
    ($cs:expr, $key:tt, $value:tt, $flag:expr, $shift:expr) => {
        if costructures_get_btree_range!($cs, $key, $flag, $shift) != Some($value) {
            let key = $key.0 | $flag << $shift;
            $cs.map.insert(key, $value);
        }
    }
}

macro_rules! costructures_set_btree_exact {
    ($cs:expr, $key:tt, $value:tt, $flag:expr, $shift:expr, $type:ident) => {
        let key = $key.0 | $flag << $shift;

        let value = match $value {
            Some(value) => {
                if $type::default().add(&LocalIndex($key.0)) == value {
                    $cs.map.remove(&key);
                    return
                } else {
                    let offset = $type::sub(&value, &$key);
                    offset.0 as usize
                }
            },
            None => 0,
        };

        $cs.map.insert(key, value);
    }
}

///
/// Optimization suggested in the original paper by storing all four metadata in one sorted map
/// the types of values are discerned by the two most significant bits in the integer key
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub(crate) struct Costructures<A> {
    map: BTreeMap<usize, usize>,
    dummy: PhantomData<A>,
}

impl<A> Costructures<A> {
    pub(crate) fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            dummy: PhantomData::default(),
        }
    }

    const RNI_FLAG: usize = 0;
    const RNI_SHIFT: usize = 0;
    const RR_FLAG: usize = 1;
    const RR_SHIFT: usize = mem::size_of::<usize>() * 8 - 2;
    const A_FLAG: usize = 1;
    const A_SHIFT: usize = mem::size_of::<usize>() * 8 - 1;
    const II_FLAG: usize = 3;
    const II_SHIFT: usize = mem::size_of::<usize>() * 8 - 2;

    const DEMASK: usize = !(Self::II_FLAG << Self::II_SHIFT);

    pub(crate) fn get_next_index(&self, key: &LocalIndex) -> Option<LocalIndex> {
        let value = costructures_get_btree_exact!(self, key, Self::RNI_FLAG, Self::RNI_SHIFT);
        Self::process_relative(key, value, RelativeNextIndex)
    }

    pub(crate) fn get_reference(&self, key: &LocalIndex) -> Option<LocalIndex> {
        let value = costructures_get_btree_exact!(self, key, Self::RR_FLAG, Self::RR_SHIFT);
        Self::process_relative(key, value, RelativeReference)
    }

    fn process_relative<O>(key: &LocalIndex, value: Option<usize>, maker: impl FnOnce(isize) -> O) -> Option<LocalIndex>
        where
            O: Offset<LocalIndex>,
    {
        let value = match value {
            Some(value) => value,
            _ => return Some(O::default().add(&key)),
        };

        // for some reason 0 isn't a valid value, for data compaction 0 is treated as None
        if value == 0 {
            return None;
        }

        let i = value as isize;
        Some(maker(i).add(key))
    }

    pub(crate) fn set_next_index(&mut self, key: LocalIndex, value: Option<LocalIndex>) {
        costructures_set_btree_exact!(self, key, value, Self::RNI_FLAG, Self::RNI_SHIFT, RelativeNextIndex);
    }

    pub(crate) fn set_reference(&mut self, key: LocalIndex, value: Option<LocalIndex>) {
        costructures_set_btree_exact!(self, key, value, Self::RR_FLAG, Self::RR_SHIFT, RelativeReference);
    }

    pub(crate) fn get_index_shift(&self, key: &LocalIndex) -> Option<IndexShift> {
        let value = costructures_get_btree_range!(self, key, Self::II_FLAG, Self::II_SHIFT)?;
        Some(IndexShift(value))
    }

    pub(crate) fn set_index_shift(&mut self, key: LocalIndex, value: IndexShift) {
        let value = value.0;
        costructures_set_btree_range!(self, key, value, Self::II_FLAG, Self::II_SHIFT)
    }
}

impl<A: Author> Costructures<A> {
    pub(crate) fn get_author(&self, key: &LocalIndex) -> Option<A> {
        costructures_get_btree_range!(self, key, Self::A_FLAG, Self::A_SHIFT).map(A::from)
    }

    pub(crate) fn set_author(&mut self, key: LocalIndex, value: A) {
        let value = value.as_usize();
        costructures_set_btree_range!(self, key, value, Self::A_FLAG, Self::A_SHIFT)
    }
}

impl<A> Debug for Costructures<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.map
                .range(..Self::RR_FLAG << Self::RR_SHIFT)
                .map(|(k, v)| (k, if *v != 0 { Some(RelativeNextIndex(*v as isize)) } else { None })))
            .entries(self.map
                .range(Self::RR_FLAG << Self::RR_SHIFT..Self::A_FLAG << Self::A_SHIFT)
                .map(|(k, v)| (k & Self::DEMASK, if *v != 0 { Some(RelativeReference(*v as isize)) } else { None })))
            .entries(self.map
                .range(Self::A_FLAG << Self::A_SHIFT .. Self::II_FLAG << Self::II_SHIFT)
                .map(|(k, v)| (k & Self::DEMASK, format!("Author({})", *v))))
            .entries(self.map
                .range(Self::II_FLAG << Self::II_SHIFT..)
                .map(|(k, v)| (k & Self::DEMASK, IndexShift(*v))))
            .finish()
    }
}

#[cfg(test)]
mod costructures_tests {
    use super::*;

    type Map = Costructures<usize>;

    #[test]
    fn set_and_get() {
        let mut map = Map::new();
        map.set_author(LocalIndex(10), 0);
        assert_eq!(None, map.get_author(&LocalIndex(5)));
        assert_eq!(Some(0), map.get_author(&LocalIndex(10)));
        assert_eq!(Some(0), map.get_author(&LocalIndex(15)));
        assert_eq!(None, map.get_index_shift(&LocalIndex(15)));

        map.set_next_index(LocalIndex(42), None);
        assert_eq!(None, map.get_next_index(&LocalIndex(42)));

        map.set_next_index(LocalIndex(42), Some(LocalIndex(50)));
        map.set_next_index(LocalIndex(50), Some(LocalIndex(1)));
        assert_eq!(Some(LocalIndex(50)), map.get_next_index(&LocalIndex(42)));
        assert_eq!(Some(LocalIndex(1)), map.get_next_index(&LocalIndex(50)));

        assert_eq!(Some(LocalIndex(1)), map.get_next_index(&LocalIndex(0)));
    }

    #[test]
    fn test_missing_compaction() {
        let mut m1 = Map::new();
        let mut m2 = Map::new();
        m1.set_index_shift(LocalIndex(20), IndexShift(2));
        m2.set_index_shift(LocalIndex(20), IndexShift(2));
        assert_eq!(m1, m2);

        m1.set_index_shift(LocalIndex(10), IndexShift(1));
        m1.set_index_shift(LocalIndex(15), IndexShift(1));
        m2.set_index_shift(LocalIndex(15), IndexShift(1));
        m2.set_index_shift(LocalIndex(10), IndexShift(1));
        assert_ne!(m1, m2);
    }
}