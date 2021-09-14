//! # Chronofold
//!
//! Chronofold is a conflict-free replicated data structure (a.k.a. *CRDT*) for
//! versioned text.
//!
//! This crate aims to offer a fast implementation with an easy-to-use
//! `Vec`-like API. It should be near impossible to shoot yourself in the foot
//! and end up with corrupted or lost data.
//!
//! **Note:** We are not there yet! While this implementation should be
//! correct, it is not yet optimized for speed and memory usage. The API might
//! see some changes as we continue to explore different use cases.
//!
//! This implementation is based on ideas published in the paper ["Chronofold:
//! a data structure for versioned text"][paper] by Victor Grishchenko and
//! Mikhail Patrakeev. If you look for a formal introduction to what a
//! chronofold is, reading that excellent paper is highly recommended!
//!
//! [paper]: https://arxiv.org/abs/2002.09511
//!
//! # Example usage
//!
//! ```rust
//! use chronofold::{Chronofold, LocalIndex, Op};
//!
//! type AuthorId = &'static str;
//!
//! // Alice creates a chronofold on her machine, makes some initial changes
//! // and sends a copy to Bob.
//! let mut cfold_a = Chronofold::<AuthorId, char>::default();
//! cfold_a.session("alice").extend("Hello chronfold!".chars());
//! let mut cfold_b = cfold_a.clone();
//!
//! // Alice adds some more text, ...
//! let ops_a: Vec<Op<AuthorId, char>> = {
//!     let mut session = cfold_a.session("alice");
//!     session.splice(
//!         LocalIndex(16)..LocalIndex(16),
//!         " - a data structure for versioned text".chars(),
//!     );
//!     session.iter_ops().map(Op::cloned).collect()
//! };
//!
//! // ... while Bob fixes a typo.
//! let ops_b: Vec<Op<AuthorId, char>> = {
//!     let mut session = cfold_b.session("bob");
//!     session.insert_after(LocalIndex(11), 'o');
//!     session.iter_ops().map(Op::cloned).collect()
//! };
//!
//! // Now their respective states have diverged.
//! assert_eq!(
//!     "Hello chronfold - a data structure for versioned text!",
//!     format!("{}", cfold_a),
//! );
//! assert_eq!("Hello chronofold!", format!("{}", cfold_b));
//!
//! // As soon as both have seen all ops, their states have converged.
//! for op in ops_a {
//!     cfold_b.apply(op).unwrap();
//! }
//! for op in ops_b {
//!     cfold_a.apply(op).unwrap();
//! }
//! let final_text = "Hello chronofold - a data structure for versioned text!";
//! assert_eq!(final_text, format!("{}", cfold_a));
//! assert_eq!(final_text, format!("{}", cfold_b));
//! ```

// As we only have a handful of public items, we've decided to re-export
// everything in the crate root and keep our internal module structure
// private. This keeps things simple for our users and gives us more
// flexibility in restructuring the crate.
mod change;
mod distributed;
mod error;
mod fmt;
mod index;
mod internal;
mod iter;
mod offsetmap;
mod rangemap;
mod session;
mod version;
mod costructures;

pub use crate::change::*;
use crate::costructures::Costructures;
pub use crate::distributed::*;
pub use crate::error::*;
pub use crate::fmt::*;
pub use crate::index::*;
pub use crate::iter::*;
pub use crate::session::*;
pub use crate::version::*;

use crate::index::{IndexShift, RelativeNextIndex, RelativeReference};
use crate::offsetmap::OffsetMap;
use crate::rangemap::RangeFromMap;

#[cfg(feature = "serde")]
#[macro_use]
extern crate serde;

/// A conflict-free replicated data structure for versioned sequences.
///
/// # Terminology
///
/// A chronofold can be regarded either as a log of changes or as a sequence of
/// elements. These two viewpoints require distinct terminology:
///
/// - A *log index* is a 0-based index in the log of changes. This indices are
///   stable (i.e. they stay the same after edits), but are subjective for
///   each author.
/// - An *element* is a visible (not yet deleted) value of type `T`.
/// - *Log order* refers to the chronological order in which changes were
///   added to the log. This order is subjective for each author.
/// - *Causal order* refers to the order of the linked list.
///
/// # Editing
///
/// You can edit a chronofold in two ways: Either by applying [`Op`]s, or by
/// creating a [`Session`] which has a `Vec`-like API.
///
/// # Indexing
///
/// Like [`Vec`], the `Chronofold` type allows to access values by index,
/// because it implements the [`Index`] trait. The same rules apply:
/// out-of-bound indexes cause panics, and you can use `get` to check whether
/// the index exists.
///
/// [`Vec`]: https://doc.rust-lang.org/std/vec/struct.Vec.html
/// [`Index`]: https://doc.rust-lang.org/std/ops/trait.Index.html
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Chronofold<A, T> {
    log: Vec<Change<T>>,
    root: LocalIndex,
    #[cfg_attr(
        feature = "serde",
        serde(bound(
            serialize = "Version<A>: serde::Serialize",
            deserialize = "Version<A>: serde::Deserialize<'de>"
        ))
    )]
    version: Version<A>,

    costructures: Costructures<A>,
}

impl<A: Author, T> Chronofold<A, T> {
    /// Constructs a new, empty chronofold.
    pub fn new(author: A) -> Self {
        let root_idx = LocalIndex(0);
        let mut version = Version::default();
        version.inc(&Timestamp::new(AuthorIndex(0), author));
        let mut costructures = Costructures::new();
        costructures.set_next_index(root_idx, None);
        costructures.set_author(root_idx, author);
        costructures.set_index_shift(root_idx, IndexShift(0));
        costructures.set_reference(root_idx, None);
        Self {
            log: vec![Change::Root],
            root: LocalIndex(0),
            version,
            costructures,
        }
    }

    fn get_next_index(&self, index: &LocalIndex) -> Option<LocalIndex> {
        self.costructures.get_next_index(index)
    }

    fn get_author(&self, index: &LocalIndex) -> Option<A> {
        self.costructures.get_author(index)
    }

    fn get_index_shift(&self, index: &LocalIndex) -> Option<IndexShift> {
        self.costructures.get_index_shift(index)
    }

    fn get_reference(&self, index: &LocalIndex) -> Option<LocalIndex> {
        self.costructures.get_reference(index)
    }

    fn set_next_index(&mut self, index: LocalIndex, value: Option<LocalIndex>) {
        self.costructures.set_next_index(index, value);
    }

    fn set_author(&mut self, index: LocalIndex, value: A) {
        self.costructures.set_author(index, value);
    }

    fn set_index_shift(&mut self, index: LocalIndex, value: IndexShift) {
        self.costructures.set_index_shift(index, value);
    }

    fn set_reference(&mut self, index: LocalIndex, value: Option<LocalIndex>) {
        self.costructures.set_reference(index, value);
    }

    /// Returns `true` if the chronofold contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of elements in the chronofold.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Returns a reference to a change in the chronofold's log.
    ///
    /// If `index` is out of bounds, `None` is returned.
    pub fn get(&self, index: LocalIndex) -> Option<&Change<T>> {
        self.log.get(index.0)
    }

    /// Creates an editing session for a single author.
    pub fn session(&mut self, author: A) -> Session<'_, A, T> {
        Session::new(author, self)
    }

    pub fn log_index(&self, timestamp: &Timestamp<A>) -> Option<LocalIndex> {
        (timestamp.idx.0 .. self.log.len())
            .map(LocalIndex)
            .find(|&index| self.timestamp(index).as_ref() == Some(timestamp))
    }

    pub fn timestamp(&self, index: LocalIndex) -> Option<Timestamp<A>> {
        let shift = self.get_index_shift(&index)?;
        let author = self.get_author(&index)?;
        Some(Timestamp::new(&index - &shift, author))
    }

    /// Applies an op to the chronofold.
    pub fn apply<V>(&mut self, op: Op<A, V>) -> Result<(), ChronofoldError<A, V>>
    where
        V: IntoLocalValue<A, T>,
    {
        // Check if an op with the same id was applied already.
        // TODO: Consider adding an `apply_unchecked` variant to skip this
        // check.
        if self.log_index(&op.id).is_some() {
            return Err(ChronofoldError::ExistingTimestamp(op));
        }

        // We rely on indices in timestamps being greater or equal than their
        // indices in every local log. This means we cannot apply an op not
        // matching this constraint, even if we know the reference.
        if op.id.idx.0 > self.log.len() {
            return Err(ChronofoldError::FutureTimestamp(op));
        }

        use OpPayload::*;
        let (reference, change) = match op.payload {
            Root => {
                (None, Change::Root)
            }
            Insert(Some(t), value) => match self.log_index(&t) {
                Some(reference) =>
                    (Some(reference),
                        Change::Insert(value.into_local_value(self))),
                None => return Err(ChronofoldError::UnknownReference(Op::insert(
                    op.id,
                    Some(t),
                    value,
                ))),
            },
            Insert(None, value) => {
                (None, Change::Insert(value.into_local_value(self)))
            }
            Delete(t) => match self.log_index(&t) {
                Some(reference) =>
                    (Some(reference), Change::Delete),
                None => return Err(ChronofoldError::UnknownReference(op)),
            },
        };

        self.apply_change(op.id, reference, change);
        Ok(())
    }
}

impl<A: Author + Default, T> Default for Chronofold<A, T> {
    fn default() -> Self {
        Self::new(A::default())
    }
}
