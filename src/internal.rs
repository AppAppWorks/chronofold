use crate::index::{IndexShift, RelativeNextIndex};
use crate::offsetmap::Offset;
use crate::{Author, Change, Chronofold, LocalIndex, Timestamp, AuthorIndex};

use std::matches;

impl<A: Author, T> Chronofold<A, T> {
    pub(crate) fn next_log_index(&self) -> LocalIndex {
        LocalIndex(self.log.len())
    }

    /// find the would-be reference for this change to be inserted
    pub(crate) fn find_predecessor(
        &self,
        id: Timestamp<A>,
        reference: Option<LocalIndex>,
        change: &Change<T>,
    ) -> Option<LocalIndex> {
        match (reference, change) {
            (_, Change::Delete) => reference, // deletes have priority
            (None, Change::Root) => reference,
            (_, Change::Root) => {
                // Roots cannot reference other entries.
                // XXX: Should we cover this by the type system?
                unreachable!()
            }
            (Some(reference), _change) => {
                self.iter_log_indices_causal_range(reference..)
                    // finding preemptive siblings
                    .filter(|(_, i)| self.get_reference(i) == Some(reference))
                    .filter(|(c, i)|
                        matches!(c, Change::Delete) || self.timestamp(*i).unwrap() > id
                    )
                    .last()
                    .map_or_else(|| Some(reference),
                                 |(_, idx)| self.iter_subtree(idx).last(),
                    )
            }
            (None, _change) => {
                // Non-roots have to reference another entry.
                // XXX: Should we cover this by the type system?
                unreachable!()
            }
        }
    }

    pub(crate) fn apply_change(
        &mut self,
        id: Timestamp<A>,
        reference: Option<LocalIndex>,
        change: Change<T>,
    ) -> LocalIndex {
        // Find the predecessor to `op`.
        let predecessor = self.find_predecessor(id, reference, &change);

        // Set the predecessor's next index to our new change's index while
        // keeping its previous next index for ourselves.
        let new_index = LocalIndex(self.log.len());
        // Inserting another root will result in two disjunct subsequences,
        // so next_index non-null only if predecessor non-null
        let next_index = predecessor.and_then(|idx| {
            let next_index = self.get_next_index(&idx);
            self.set_next_index(idx, Some(new_index));
            next_index
        });

        // Append to the chronofold's log and secondary logs.
        self.log.push(change);
        self.set_next_index(new_index, next_index);
        self.set_author(new_index, id.author);
        self.set_index_shift(new_index, IndexShift(new_index.0 - (id.idx).0));
        self.set_reference(new_index, reference);

        // Increment version.
        self.version.inc(&id);

        new_index
    }

    /// Applies consecutive local changes.
    ///
    /// For local changes the following optimizations can be applied:
    /// - id equals (log index, author)
    /// - predecessor always equals reference (no preemptive siblings)
    /// - next index has to be set only for the first and the last change
    pub(crate) fn apply_local_changes(
        &mut self,
        author: A,
        reference: LocalIndex,
        changes: impl IntoIterator<Item = Change<T>>,
    ) -> Option<LocalIndex>
    {
        let mut last_id = None;
        let mut last_next_index = None;

        let mut predecessor = self.find_last_delete(reference).unwrap_or(reference);

        let mut changes = changes.into_iter();
        if let Some(first_change) = changes.next() {
            let new_index = LocalIndex(self.log.len());
            let id = Timestamp::new(AuthorIndex(new_index.0), author);
            last_id = Some(id);

            // Set the predecessors next index to our new change's index while
            // keeping it's previous next index for ourselves.
            last_next_index = self.get_next_index(&predecessor);
            self.set_next_index(predecessor, Some(new_index));

            self.log.push(first_change);
            self.set_author(new_index, author);
            self.set_index_shift(new_index, IndexShift(0));
            self.set_reference(new_index, Some(predecessor));

            predecessor = new_index;
        }

        for change in changes {
            let new_index = RelativeNextIndex::default().add(&predecessor);
            let id = Timestamp::new(AuthorIndex(new_index.0), author);
            last_id = Some(id);

            // Append to the chronofold's log and secondary logs.
            self.log.push(change);

            predecessor = new_index;
        }
        
        let id = last_id?;
        self.set_next_index(LocalIndex(id.idx.0), last_next_index);
        self.version.inc(&id);
        Some(LocalIndex(id.idx.0))
    }

    pub(crate) fn find_last_delete(&self, reference: LocalIndex) -> Option<LocalIndex> {
        self.iter_log_indices_causal_range(reference..)
            .skip(1)
            .filter(|(c, idx)| {
                matches!(c, Change::Delete) && self.get_reference(idx) == Some(reference)
            })
            .last()
            .map(|(_, idx)| idx)
    }
}
