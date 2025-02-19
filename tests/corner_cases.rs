use chronofold::{Chronofold, LocalIndex, Op, Session};

#[test]
fn concurrent_insertions() {
    // Both insert after the same character:
    assert_concurrent_eq(
        "012!",
        "0",
        |s| {
            s.extend("!".chars());
        },
        |s| {
            s.extend("12".chars());
        },
    );
}

#[test]
fn concurrent_deletions() {
    // Both delete the same single character:
    assert_concurrent_eq(
        "fobar",
        "foobar",
        |s| {
            s.remove(LocalIndex(2));
        },
        |s| {
            s.remove(LocalIndex(2));
        },
    );
}

#[test]
fn concurrent_replacements() {
    // Both replace the same substring:
    assert_concurrent_eq(
        "foobaz123",
        "foobar",
        |s| {
            s.splice(LocalIndex(4).., "123".chars());
        },
        |s| {
            s.splice(LocalIndex(4).., "baz".chars());
        },
    );
}

#[test]
fn concurrent_insertion_deletion() {
    // Alice inserts after a character that is concurrently deleted by Bob.

    // Equal log indices for the conflicting edits:
    assert_concurrent_eq(
        "0!",
        "01",
        |s| {
            s.insert_after(LocalIndex(2), '!');
        },
        |s| {
            s.remove(LocalIndex(2));
        },
    );

    // Insert's log index is greater:
    assert_concurrent_eq(
        "0!23",
        "01",
        |s| {
            s.extend("23".chars());
            s.insert_after(LocalIndex(2), '!');
        },
        |s| {
            s.remove(LocalIndex(2));
        },
    );

    // Delete's log index is greater:
    assert_concurrent_eq(
        "023!",
        "01",
        |s| {
            s.insert_after(LocalIndex(2), '!');
        },
        |s| {
            s.extend("23".chars());
            s.remove(LocalIndex(2));
        },
    );
}

#[test]
fn insert_referencing_deleted_element() {
    let mut cfold = Chronofold::<u8, char>::default();
    let mut session = cfold.session(1);
    let idx = session.push_back('!');
    session.clear();
    session.insert_after(idx, '?');
    assert_eq!("?", format!("{}", cfold));
}

fn assert_concurrent_eq<F, G>(expected: &str, initial: &str, mutate_left: F, mutate_right: G)
where
    F: FnOnce(&mut Session<u8, char>) -> (),
    G: FnOnce(&mut Session<u8, char>) -> (),
{
    let mut cfold_left = Chronofold::<u8, char>::default();
    cfold_left.session(1).extend(initial.chars());
    let mut cfold_right = cfold_left.clone();

    let ops_left: Vec<_> = {
        let mut session = cfold_left.session(1);
        mutate_left(&mut session);
        session.iter_ops().map(Op::cloned).collect()
    };
    let ops_right: Vec<_> = {
        let mut session = cfold_right.session(2);
        mutate_right(&mut session);
        session.iter_ops().map(Op::cloned).collect()
    };

    for op in ops_left {
        cfold_right.apply(op).unwrap();
    }
    for op in ops_right {
        cfold_left.apply(op).unwrap();
    }

    assert_eq!(
        expected,
        format!("{}", cfold_left),
        "Left ops:\n{:#?}",
        cfold_left.iter_ops(..).collect::<Vec<Op<_, &char>>>(),
    );
    assert_eq!(
        expected,
        format!("{}", cfold_right),
        "Right ops:\n{:#?}",
        cfold_right.iter_ops(..).collect::<Vec<Op<_, &char>>>()
    );
}
