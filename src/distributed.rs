//! Distributed primitives.

use std::fmt;

use crate::{AuthorIndex, Chronofold};

/// A trait alias to reduce redundancy in type declarations.
pub trait Author:
    PartialEq + Eq + PartialOrd + Ord + Clone + Copy + fmt::Debug + fmt::Display
{
    fn from(raw: usize) -> Self;
    fn as_usize(&self) -> usize;
}

macro_rules! impl_for_author {
    ($type:ident) => {
        impl Author for $type {
            fn from(raw: usize) -> Self {
                raw as Self
            }

            fn as_usize(&self) -> usize {
                *self as usize
            }
        }
    };
}

impl_for_author!(u8);
impl_for_author!(usize);

/// An ordered pair of the author's index and the author.
///
/// The lexicographic order of timestamps forms an arbitrary total order, that
/// is consistent with cause-effect ordering. That is, if a timestamp is
/// greater than another, its associated event either happened after the other
/// or was concurrent.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Timestamp<A> {
    pub idx: AuthorIndex,
    pub author: A,
}

impl<A> Timestamp<A> {
    pub fn new(idx: AuthorIndex, author: A) -> Self {
        Self { idx, author }
    }
}

impl<A: fmt::Display> fmt::Display for Timestamp<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<{}, {}>", self.idx, self.author)
    }
}

/// An operation is the unit of change in the distributed context.
///
/// Ops are independent of the subjective orders in the chronofolds'
/// logs. Different authors exchange ops to keep their local replicas
/// synchronized.
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Op<A, T> {
    pub id: Timestamp<A>,
    pub payload: OpPayload<A, T>,
}

impl<A, T> Op<A, T> {
    pub fn new(id: Timestamp<A>, payload: OpPayload<A, T>) -> Self {
        Self { id, payload }
    }

    pub fn root(id: Timestamp<A>) -> Self {
        Op::new(id, OpPayload::Root)
    }

    pub fn insert(id: Timestamp<A>, reference: Option<Timestamp<A>>, value: T) -> Self {
        Op::new(id, OpPayload::Insert(reference, value))
    }

    pub fn delete(id: Timestamp<A>, reference: Timestamp<A>) -> Self {
        Op::new(id, OpPayload::Delete(reference))
    }
}

impl<A, T: Clone> Op<A, &T> {
    /// Maps an Op<A, &T> to an Op<A, T> by cloning the payload.
    pub fn cloned(self) -> Op<A, T> {
        Op {
            id: self.id,
            payload: self.payload.cloned(),
        }
    }
}

/// The payload of an operation.
///
/// Ops don't contain `Change<T>` directly, as these can contain information
/// that is only meaningful within the context of the local chronofold. E.g. a
/// change may refer to another change by log index, which has to be replaced
/// by a timestamp in the distributed operation.
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum OpPayload<A, T> {
    Root,
    Insert(Option<Timestamp<A>>, T),
    Delete(Timestamp<A>),
}

impl<A, T> OpPayload<A, T> {
    pub fn reference(&self) -> Option<&Timestamp<A>> {
        use OpPayload::*;
        match self {
            Root => None,
            Insert(reference, _) => reference.as_ref(),
            Delete(reference) => Some(reference),
        }
    }
}

impl<A, T: Clone> OpPayload<A, &T> {
    pub fn cloned(self) -> OpPayload<A, T> {
        use OpPayload::*;
        match self {
            Root => Root,
            Insert(reference, t) => Insert(reference, t.clone()),
            Delete(reference) => Delete(reference),
        }
    }
}

pub trait IntoLocalValue<A, LocalValue> {
    fn into_local_value(self, chronofold: &Chronofold<A, LocalValue>) -> LocalValue;
}

pub trait FromLocalValue<'a, A, LocalValue> {
    fn from_local_value(source: &'a LocalValue, chronofold: &Chronofold<A, LocalValue>) -> Self;
}

impl<A, T, V> IntoLocalValue<A, T> for V
where
    V: Into<T>,
{
    fn into_local_value(self, _chronofold: &Chronofold<A, T>) -> T {
        self.into()
    }
}

impl<'a, A, T> FromLocalValue<'a, A, T> for &'a T {
    fn from_local_value(source: &'a T, _chronofold: &Chronofold<A, T>) -> Self {
        source
    }
}
