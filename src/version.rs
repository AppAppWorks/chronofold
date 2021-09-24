use std::cmp::Ordering;

use crate::{Author, Chronofold, FromLocalValue, LocalIndex, Op, Timestamp, AuthorIndex, LogIndex};

/// A vector clock representing the chronofold's version.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Version<A> {
    log_indices: Vec<Timestamp<A>>,
}

impl<A: Author> Version<A> {
    /// Constructs a new, empty version.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increments the version using a timestamp.
    pub fn inc(&mut self, timestamp: &Timestamp<A>) {
        match self.log_indices
            .binary_search_by(|t| t.author.cmp(&timestamp.author)) {
            Ok(idx) => self.log_indices[idx].idx.take_max(&timestamp.idx),
            Err(idx) => self.log_indices.insert(idx, *timestamp),
        };
    }

    /// Returns an iterator over the timestamps in this version.
    pub fn iter(&self) -> impl Iterator<Item = Timestamp<A>> + '_ {
        self.log_indices.iter().map(Timestamp::clone)
    }

    /// Returns the version's log index for `author`.
    pub fn get(&self, author: &A) -> Option<AuthorIndex> {
        let idx = self.log_indices
            .binary_search_by(|t| t.author.cmp(author)).ok()?;
        Some(self.log_indices[idx].idx)
    }
}

impl<A: Author> Default for Version<A> {
    fn default() -> Self {
        Self {
            log_indices: Vec::new(),
        }
    }
}

impl<A: Author> PartialOrd for Version<A> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let gt = |lhs: &Self, rhs: &Self| {
            rhs.log_indices.iter().all(|ref t| {
                lhs.get(&t.author)
                    .map(|lhs_idx| lhs_idx >= t.idx)
                    .unwrap_or(false)
            })
        };

        if self == other {
            Some(Ordering::Equal)
        } else if gt(self, other) {
            Some(Ordering::Greater)
        } else if gt(other, self) {
            Some(Ordering::Less)
        } else {
            None
        }
    }
}

impl<A: Author, T> Chronofold<A, T> {
    /// Returns a vector clock representing the version of this chronofold.
    pub fn version(&self) -> &Version<A> {
        &self.version
    }

    /// Returns an iterator over ops newer than the given version in log order.
    pub fn iter_newer_ops<'a, V>(
        &'a self,
        version: &'a Version<A>,
    ) -> impl Iterator<Item = Op<A, V>> + 'a
    where
        V: FromLocalValue<'a, A, T> + 'a,
    {
        // TODO: Don't iterate over all ops in cases where that is not
        // necessary.
        self.iter_ops(..)// O(nlog(n))
            .filter(move |op| match version.log_indices
                .binary_search_by(|t| t.author.cmp(&op.id.author)) {
                Err(_) => true,
                Ok(idx) => op.id.idx > version.log_indices[idx].idx,
            })
    }
}

// TODO: Figure out how to derive Serialize/Deserialize only for `A: Ord`.
#[cfg(feature = "serde")]
mod serde {
    use super::Version;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::cmp::Ord;
    use std::collections::BTreeMap;

    impl<A> Serialize for Version<A>
    where
        A: Serialize + Ord,
    {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.log_indices.serialize(serializer)
        }
    }

    impl<'de, A> Deserialize<'de> for Version<A>
    where
        A: Deserialize<'de> + Ord,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            Ok(Self {
                log_indices: Vec::deserialize(deserializer)?,
            })
        }
    }
}
