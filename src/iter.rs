use std::collections::HashSet;
use std::marker::PhantomData;
use std::matches;
use std::ops::{Bound, Range, RangeBounds};

use crate::{Author, Change, Chronofold, FromLocalValue, LocalIndex, Op, OpPayload};

impl<A: Author, T> Chronofold<A, T> {
    /// Returns an iterator over the log indices in causal order.
    ///
    /// TODO: The name is a bit unwieldy. I'm reluctant to add it to the public
    /// API before giving it more thought.
    pub(crate) fn iter_log_indices_causal_range<R>(&self, range: R) -> CausalIter<'_, A, T>
    where
        R: RangeBounds<LocalIndex>,
    {
        let mut current = match range.start_bound() {
            Bound::Unbounded => self.index_after(self.root),
            Bound::Included(idx) => Some(*idx),
            Bound::Excluded(idx) => self.index_after(*idx),
        };
        if let Some((Some(Change::Root), idx)) = current.map(|idx| (self.get(idx), idx)) {
            current = self.index_after(idx);
        }
        let first_excluded = match range.end_bound() {
            Bound::Unbounded => None,
            Bound::Included(idx) => self.index_after(*idx),
            Bound::Excluded(idx) => Some(*idx),
        };
        CausalIter {
            cfold: self,
            current,
            first_excluded,
        }
    }

    /// Returns an iterator over a subtree.
    ///
    /// The first item is always `root`.
    pub(crate) fn iter_subtree(&self, root: LocalIndex) -> impl Iterator<Item = LocalIndex> + '_ {
        let mut subtree: HashSet<LogIndex> = HashSet::new();
        self.iter_log_indices_causal_range(root..)
            .filter_map(move |(_, idx)| {
                if idx == root || subtree.contains(&self.references.get(&idx)?) {
                    subtree.insert(idx);
                    Some(idx)
                } else {
                    None
                }
            })
    }

    /// Returns an iterator over elements and their log indices in causal order.
    pub fn iter(&self) -> Iter<A, T> {
        self.iter_range(..)
    }

    /// Returns an iterator over elements and their log indices in causal order.
    pub fn iter_range<R>(&self, range: R) -> Iter<A, T>
    where
        R: RangeBounds<LocalIndex>,
    {
        let mut causal_iter = self.iter_log_indices_causal_range(range);
        let current = causal_iter.next();
        Iter {
            causal_iter,
            current,
        }
    }

    /// Returns an iterator over elements in causal order.
    pub fn iter_elements(&self) -> impl Iterator<Item = &T> {
        self.iter().map(|(v, _)| v)
    }

    /// Returns an iterator over changes in log order.
    pub fn iter_changes(&self) -> impl Iterator<Item = &Change<T>> {
        self.log.iter()
    }

    /// Returns an iterator over ops in log order.
    pub fn iter_ops<'a, R, V>(&'a self, range: R) -> Ops<'a, A, T, V>
    where
        R: RangeBounds<LocalIndex> + 'a,
        V: FromLocalValue<'a, A, T>,
    {
        let oob = LocalIndex(self.log.len());
        let start = match range.start_bound() {
            Bound::Unbounded => LocalIndex(0),
            Bound::Included(idx) => *idx,
            Bound::Excluded(idx) => self.index_after(*idx).unwrap_or(oob),
        }
        .0;
        let end = match range.end_bound() {
            Bound::Unbounded => oob,
            Bound::Included(idx) => self.index_after(*idx).unwrap_or(oob),
            Bound::Excluded(idx) => *idx,
        }
        .0;
        Ops {
            cfold: self,
            idx_iter: start..end,
            _op_value: PhantomData,
        }
    }
}

pub(crate) struct CausalIter<'a, A, T> {
    cfold: &'a Chronofold<A, T>,
    current: Option<LocalIndex>,
    first_excluded: Option<LocalIndex>,
}

impl<'a, A: Author, T> Iterator for CausalIter<'a, A, T> {
    type Item = (&'a Change<T>, LocalIndex);

    fn next(&mut self) -> Option<Self::Item> {
        match self.current.take() {
            Some(current) if Some(current) != self.first_excluded => {
                self.current = self.cfold.index_after(current);
                Some((&self.cfold.log[current.0], current))
            }
            _ => None,
        }
    }
}

/// An iterator over the elements of a chronofold.
///
/// This struct is created by the `iter` and `iter_range` methods on
/// `Chronofold`. See its documentation for more.
pub struct Iter<'a, A, T> {
    causal_iter: CausalIter<'a, A, T>,
    current: Option<(&'a Change<T>, LocalIndex)>,
}

impl<'a, A: Author, T> Iterator for Iter<'a, A, T> {
    type Item = (&'a T, LocalIndex);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (skipped, next) =
                skip_while(&mut self.causal_iter, |(c, _)| matches!(c, Change::Delete));
            if skipped == 0 {
                // the current item is not deleted
                break match self.current.take() {
                    None => None,
                    Some((Change::Insert(v), idx)) => {
                        self.current = next;
                        Some((v, idx))
                    }
                    _ => unreachable!(),
                }
            } else {
                // the current item is deleted
                self.current = next;
            }
        }
    }
}

/// An iterator over ops representing a chronofold's changes.
///
/// This struct is created by the `iter_ops` method on `Chronofold`. See its
/// documentation for more.
pub struct Ops<'a, A, T, V> {
    cfold: &'a Chronofold<A, T>,
    idx_iter: Range<usize>,
    _op_value: PhantomData<V>,
}

impl<'a, A, T, V> Iterator for Ops<'a, A, T, V>
where
    A: Author,
    V: FromLocalValue<'a, A, T>,
{
    type Item = Op<A, V>;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = LocalIndex(self.idx_iter.next()?);
        let id = self
            .cfold
            .timestamp(idx)
            .expect("timestamps of already applied ops have to exist");
        let reference = self.cfold.get_reference(&idx).map(|r| {
            self.cfold
                .timestamp(r)
                .expect("references of already applied ops have to exist")
        });
        let payload = match &self.cfold.log[idx.0] {
            Change::Root => OpPayload::Root,
            Change::Insert(v) => OpPayload::Insert(reference, V::from_local_value(v, self.cfold)),
            Change::Delete => OpPayload::Delete(reference.expect("deletes must have a reference")),
        };
        Some(Op::new(id, payload))
    }
}

/// Skips items where `predicate` returns true.
///
/// Note that while this works like `Iterator::skip_while`, it does not create
/// a new iterator. Instead `iter` is modified.
fn skip_while<I, P>(iter: &mut I, predicate: P) -> (usize, Option<I::Item>)
where
    I: Iterator,
    P: Fn(&I::Item) -> bool,
{
    let mut skipped = 0;
    loop {
        match iter.next() {
            Some(item) if !predicate(&item) =>
                break (skipped, Some(item)),
            None =>
                break (skipped, None),
            _ => skipped += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{AuthorIndex, Timestamp};

    use super::*;

    #[test]
    fn iter_subtree() {
        let mut cfold = Chronofold::<u8, char>::default();
        cfold.session(1).extend("013".chars());
        cfold.session(1).insert_after(LocalIndex(2), '2');
        assert_eq!(
            vec![LocalIndex(2), LocalIndex(4), LocalIndex(3)],
            cfold.iter_casual_tree(LocalIndex(2)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn iter_ops() {
        let mut cfold = Chronofold::<u8, char>::default();
        cfold.session(1).extend("Hi!".chars());
        let op0 = Op::root(Timestamp::new(AuthorIndex(0), 0));
        let op1 = Op::insert(
            Timestamp::new(AuthorIndex(1), 1),
            Some(Timestamp::new(AuthorIndex(0), 0)),
            &'H',
        );
        let op2 = Op::insert(
            Timestamp::new(AuthorIndex(2), 1),
            Some(Timestamp::new(AuthorIndex(1), 1)),
            &'i',
        );
        let op3 = Op::insert(
            Timestamp::new(AuthorIndex(3), 1),
            Some(Timestamp::new(AuthorIndex(2), 1)),
            &'!',
        );
        assert_eq!(
            vec![op0.clone(), op1.clone(), op2.clone()],
            cfold.iter_ops(..LocalIndex(3)).collect::<Vec<_>>()
        );
        assert_eq!(
            vec![op2, op3],
            cfold.iter_ops(LocalIndex(2)..).collect::<Vec<_>>()
        );
    }

    #[test]
    fn skip_while() {
        let mut iter = 2..10;
        let result = super::skip_while(&mut iter, |i| i < &7);
        assert_eq!((5, Some(7)), result);
        assert_eq!(vec![8, 9], iter.collect::<Vec<_>>());

        let mut iter2 = 2..10;
        let result = super::skip_while(&mut iter2, |i| i < &20);
        assert_eq!((8, None), result);
    }
}
