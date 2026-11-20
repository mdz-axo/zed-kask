//! Putting a conversation in order (NIP-10, NIP-22).
//!
//! A reply names its conversation's root and the note it answers in `e`
//! tags; a comment (kind 1111) names its root and parent in NIP-22's
//! uppercase and lowercase tags. Nothing here contacts a relay: given the
//! events that were found, this module works out who answers whom, and
//! which notes the conversation refers to that were not found.
//!
//! Scope: kind 1 notes and their kind 1 replies and kind 1111 comments,
//! in one reading order. A comment whose root is addressable (`A`) or an
//! author (`I`) is named as such for the server to resolve or refuse.

use std::collections::{BTreeMap, BTreeSet};

use nostr_sdk::prelude::*;

/// The most events one conversation is assembled from.
pub const MAX_THREAD_EVENTS: usize = 200;

/// The root and the parent a note names, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct References {
    /// The conversation's first note.
    pub root: Option<EventId>,
    /// The note this one answers.
    pub parent: Option<EventId>,
}

/// The kind of a NIP-22 comment.
pub const KIND_COMMENT: u16 = 1111;

/// A comment's root or parent as NIP-22 names it: an event by id, an
/// addressable event, an author, or nothing. An addressable or author
/// root carries no event id, so the server resolves or refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommentRoot {
    /// No root named.
    #[default]
    None,
    /// An event named by id: the uppercase `E` for a root, the lowercase
    /// `e` for a parent.
    Event(EventId),
    /// An addressable event (NIP-01: kind, author, `d`), named by the
    /// uppercase `A` for a root or the lowercase `a` for a parent.
    Address {
        kind: u16,
        author: PublicKey,
        d: String,
    },
    /// The root is an author (NIP-22's `I`/`i`): no event to assemble.
    Author,
}

impl CommentRoot {
    /// The event id this names, when it names one.
    fn event_id(&self) -> Option<EventId> {
        match self {
            CommentRoot::Event(id) => Some(*id),
            _ => None,
        }
    }
}

/// A comment's NIP-22 reference tags: its root and parent, and the kinds
/// it says they are (`K`/`k`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommentReferences {
    pub root: CommentRoot,
    pub parent: CommentRoot,
    pub root_kind: Option<u16>,
    pub parent_kind: Option<u16>,
}

/// Read a comment's NIP-22 tags. The first of each form wins; an
/// unparseable id or address names nothing.
pub fn comment_references(event: &Event) -> CommentReferences {
    let mut comment = CommentReferences::default();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        let Some(name) = parts.first().map(String::as_str) else {
            continue;
        };
        let value = parts.get(1).map(String::as_str);
        match name {
            "E" if comment.root == CommentRoot::None => {
                if let Some(id) = value.and_then(|value| EventId::from_hex(value).ok()) {
                    comment.root = CommentRoot::Event(id);
                }
            }
            "A" if comment.root == CommentRoot::None => {
                if let Some(address) = address_of(parts) {
                    comment.root = address;
                }
            }
            "I" if comment.root == CommentRoot::None => {
                if value.is_some() {
                    comment.root = CommentRoot::Author;
                }
            }
            "e" if comment.parent == CommentRoot::None => {
                if let Some(id) = value.and_then(|value| EventId::from_hex(value).ok()) {
                    comment.parent = CommentRoot::Event(id);
                }
            }
            "a" if comment.parent == CommentRoot::None => {
                if let Some(address) = address_of(parts) {
                    comment.parent = address;
                }
            }
            "i" if comment.parent == CommentRoot::None => {
                if value.is_some() {
                    comment.parent = CommentRoot::Author;
                }
            }
            "K" => comment.root_kind = value.and_then(|kind| kind.parse().ok()),
            "k" => comment.parent_kind = value.and_then(|kind| kind.parse().ok()),
            _ => {}
        }
    }
    comment
}

/// The comment's root, for the server to resolve: an event by id, an
/// addressable event, an author, or nothing.
pub fn comment_root_target(event: &Event) -> CommentRoot {
    comment_references(event).root
}

/// An `A`/`a` tag's addressable event coordinate `<kind>:<pubkey>:<d>` (NIP-01 / NIP-22).
fn address_of(parts: &[String]) -> Option<CommentRoot> {
    let mut coords = parts.get(1)?.splitn(3, ':');
    let kind: u16 = coords.next()?.parse().ok()?;
    let author = PublicKey::from_hex(coords.next()?).ok()?;
    let d = coords.next()?.to_string();
    Some(CommentRoot::Address { kind, author, d })
}

/// Read an event's conversation references. A kind 1 note is read the
/// way NIP-10 says: marked tags win — `root` names the root and `reply`
/// the parent; a note with only a `root` answers the root; without any
/// marker the older positional rule applies — the first `e` tag is the
/// root and the last is the parent. A tag marked `mention` is a
/// citation, not a parent.
///
/// A comment (kind 1111) is read the way NIP-22 says: the uppercase `E`
/// names the root and the lowercase `e` the parent, and a comment with
/// only `E` answers its root. An addressable or author root carries no
/// event id and is left for the server to resolve.
pub fn references(event: &Event) -> References {
    if event.kind.as_u16() == KIND_COMMENT {
        let comment = comment_references(event);
        let root = comment.root.event_id();
        return References {
            root,
            parent: comment.parent.event_id().or(root),
        };
    }
    let mut marked_root = None;
    let mut marked_reply = None;
    let mut any_marker = false;
    let mut positional: Vec<EventId> = Vec::new();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("e") {
            continue;
        }
        let Some(id) = parts.get(1).and_then(|text| EventId::from_hex(text).ok()) else {
            continue;
        };
        match parts.get(3).map(String::as_str) {
            Some("root") => {
                any_marker = true;
                marked_root.get_or_insert(id);
            }
            Some("reply") => {
                any_marker = true;
                marked_reply.get_or_insert(id);
            }
            Some("mention") => any_marker = true,
            _ => positional.push(id),
        }
    }
    if any_marker {
        References {
            root: marked_root,
            parent: marked_reply.or(marked_root),
        }
    } else {
        References {
            root: positional.first().copied(),
            parent: positional.last().copied(),
        }
    }
}

/// The root of the conversation `event` belongs to: the root it names, or
/// itself when it names none.
pub fn root_of(event: &Event) -> EventId {
    references(event).root.unwrap_or(event.id)
}

/// One note's place in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'event> {
    pub event: &'event Event,
    /// The note this one answers, as it names it.
    pub parent: Option<EventId>,
    /// How many of this note's ancestors are in the result. When
    /// `parent_missing` is set on it or on an ancestor, the true depth is
    /// greater.
    pub depth: usize,
    /// The note names a parent that was not found.
    pub parent_missing: bool,
}

/// A conversation in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled<'event> {
    /// Every event given, once: the root first when it was found, each note
    /// followed by its answers, oldest first.
    pub entries: Vec<Entry<'event>>,
    /// Notes the conversation refers to that were not found, in id order.
    pub missing: Vec<EventId>,
}

/// Order `events` as the conversation rooted at `root`.
pub fn assemble(root: EventId, events: &[Event]) -> Assembled<'_> {
    let present: BTreeSet<EventId> = events.iter().map(|event| event.id).collect();
    let mut children: BTreeMap<EventId, Vec<&Event>> = BTreeMap::new();
    let mut heads: Vec<(&Event, Option<EventId>, bool)> = Vec::new();
    let mut missing: BTreeSet<EventId> = BTreeSet::new();
    if !present.contains(&root) {
        missing.insert(root);
    }

    for event in events {
        if event.id == root {
            heads.push((event, None, false));
            continue;
        }
        match references(event).parent {
            Some(parent) if parent != event.id && present.contains(&parent) => {
                children.entry(parent).or_default().push(event);
            }
            Some(parent) => {
                if parent != event.id {
                    missing.insert(parent);
                }
                heads.push((event, Some(parent), true));
            }
            // A note that names no parent and is not the root was returned
            // by a relay for another reason; it is shown, attached to
            // nothing.
            None => heads.push((event, None, false)),
        }
    }

    let oldest_first =
        |a: &&Event, b: &&Event| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id));
    for answers in children.values_mut() {
        answers.sort_by(oldest_first);
    }
    // The root first, then the notes whose parent was not found.
    heads.sort_by(|(a, _, _), (b, _, _)| {
        (a.id != root).cmp(&(b.id != root)).then(oldest_first(a, b))
    });

    let mut entries: Vec<Entry<'_>> = Vec::with_capacity(events.len());
    let mut placed: BTreeSet<EventId> = BTreeSet::new();
    for (head, parent, parent_missing) in heads {
        let mut pending: Vec<Entry<'_>> = vec![Entry {
            event: head,
            parent,
            depth: 0,
            parent_missing,
        }];
        while let Some(entry) = pending.pop() {
            if !placed.insert(entry.event.id) {
                continue;
            }
            entries.push(entry);
            if let Some(answers) = children.get(&entry.event.id) {
                // Pushed newest first, so the oldest answer is read next.
                for answer in answers.iter().rev() {
                    pending.push(Entry {
                        event: answer,
                        parent: Some(entry.event.id),
                        depth: entry.depth + 1,
                        parent_missing: false,
                    });
                }
            }
        }
    }
    // Every event given is in the result. Two notes cannot name each other
    // as parent (an id is a hash of the note that carries it), so nothing
    // reaches here; if something did, it is shown rather than lost.
    for event in events {
        if placed.insert(event.id) {
            entries.push(Entry {
                event,
                parent: references(event).parent,
                depth: 0,
                parent_missing: true,
            });
        }
    }

    Assembled {
        entries,
        missing: missing.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(keys: &Keys, at: u64, text: &str, tags: &[&[&str]]) -> Event {
        let tags: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).expect("a tag"))
            .collect();
        EventBuilder::new(Kind::from_u16(1), text)
            .tags(tags)
            .custom_created_at(Timestamp::from_secs(at))
            .finalize(keys)
            .expect("a throwaway key signs")
    }

    fn comment(keys: &Keys, at: u64, text: &str, tags: &[&[&str]]) -> Event {
        let tags: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).expect("a tag"))
            .collect();
        EventBuilder::new(Kind::from_u16(1111), text)
            .tags(tags)
            .custom_created_at(Timestamp::from_secs(at))
            .finalize(keys)
            .expect("a throwaway key signs")
    }

    fn texts<'a>(assembled: &'a Assembled<'_>) -> Vec<(&'a str, usize)> {
        assembled
            .entries
            .iter()
            .map(|entry| (entry.event.content.as_str(), entry.depth))
            .collect()
    }

    /// expect: marked tags name the root and the parent; a lone `root`
    /// marker makes the root the parent; a `mention` is neither.
    #[test]
    fn marked_e_tags_name_the_root_and_the_parent() {
        let keys = Keys::generate();
        let root = note(&keys, 1, "root", &[]);
        let middle = note(&keys, 2, "middle", &[&["e", &root.id.to_hex(), "", "root"]]);
        assert_eq!(
            references(&middle),
            References {
                root: Some(root.id),
                parent: Some(root.id)
            }
        );

        let leaf = note(
            &keys,
            3,
            "leaf",
            &[
                &["e", &root.id.to_hex(), "", "root"],
                &["e", &middle.id.to_hex(), "", "reply"],
            ],
        );
        assert_eq!(
            references(&leaf),
            References {
                root: Some(root.id),
                parent: Some(middle.id)
            }
        );

        let citing = note(
            &keys,
            4,
            "citing",
            &[&["e", &root.id.to_hex(), "", "mention"]],
        );
        assert_eq!(references(&citing), References::default());
        assert_eq!(root_of(&citing), citing.id);
        assert_eq!(root_of(&leaf), root.id);
    }

    /// expect: without markers, the first `e` tag is the root and the last
    /// the parent (NIP-10's older positional form).
    #[test]
    fn unmarked_e_tags_are_read_by_position() {
        let keys = Keys::generate();
        let root = note(&keys, 1, "root", &[]);
        let middle = note(&keys, 2, "middle", &[&["e", &root.id.to_hex()]]);
        let leaf = note(
            &keys,
            3,
            "leaf",
            &[&["e", &root.id.to_hex()], &["e", &middle.id.to_hex()]],
        );
        assert_eq!(references(&middle).parent, Some(root.id));
        assert_eq!(
            references(&leaf),
            References {
                root: Some(root.id),
                parent: Some(middle.id)
            }
        );
    }

    /// expect: a conversation reads root first, each note followed by its
    /// answers oldest first, whatever order the relays returned them in.
    #[test]
    fn a_conversation_is_ordered_root_first_with_answers_under_their_parent() {
        let keys = Keys::generate();
        let root = note(&keys, 1, "root", &[]);
        let root_hex = root.id.to_hex();
        let first = note(&keys, 2, "first", &[&["e", &root_hex, "", "root"]]);
        let second = note(&keys, 5, "second", &[&["e", &root_hex, "", "root"]]);
        let under_first = note(
            &keys,
            9,
            "under first",
            &[
                &["e", &root_hex, "", "root"],
                &["e", &first.id.to_hex(), "", "reply"],
            ],
        );
        let events = [under_first, second, root.clone(), first];

        let assembled = assemble(root.id, &events);
        assert_eq!(
            texts(&assembled),
            [("root", 0), ("first", 1), ("under first", 2), ("second", 1)]
        );
        assert!(assembled.missing.is_empty());
        assert!(assembled.entries.iter().all(|entry| !entry.parent_missing));
    }

    /// expect: a note whose parent was not found is still shown, marked, and
    /// the parent is counted as missing once.
    #[test]
    fn a_missing_parent_is_counted_and_its_answers_are_still_shown() {
        let keys = Keys::generate();
        let root = note(&keys, 1, "root", &[]);
        let root_hex = root.id.to_hex();
        let lost = note(&keys, 2, "lost", &[&["e", &root_hex, "", "root"]]);
        let lost_hex = lost.id.to_hex();
        let orphan = |at, text| {
            note(
                &keys,
                at,
                text,
                &[
                    &["e", &root_hex, "", "root"],
                    &["e", &lost_hex, "", "reply"],
                ],
            )
        };
        let events = [orphan(4, "orphan b"), orphan(3, "orphan a"), root.clone()];

        let assembled = assemble(root.id, &events);
        assert_eq!(
            texts(&assembled),
            [("root", 0), ("orphan a", 0), ("orphan b", 0)]
        );
        assert_eq!(assembled.missing, [lost.id]);
        let marked: Vec<bool> = assembled
            .entries
            .iter()
            .map(|entry| entry.parent_missing)
            .collect();
        assert_eq!(marked, [false, true, true]);
    }

    /// expect: a comment's NIP-22 tags name its root and parent — the
    /// uppercase `E` the root, the lowercase `e` the parent, `K`/`k`
    /// their kinds — and a comment with only `E` answers its root; an
    /// addressable root (`A`) and a comment on an author (`I`) are named
    /// as such, and a kind 1 note's NIP-10 reading is unchanged.
    #[test]
    fn a_comments_nip22_tags_name_its_root_and_parent() {
        let keys = Keys::generate();
        let other = Keys::generate();
        let root = note(&keys, 1, "root", &[]);
        let root_hex = root.id.to_hex();
        let middle = note(&other, 2, "middle", &[]);
        let middle_hex = middle.id.to_hex();
        let author_hex = keys.public_key().to_hex();

        let full = comment(
            &keys,
            5,
            "full",
            &[
                &["E", root_hex.as_str()],
                &["e", middle_hex.as_str()],
                &["p", author_hex.as_str()],
                &["K", "1"],
                &["k", "1"],
            ],
        );
        let refs = comment_references(&full);
        assert_eq!(refs.root, CommentRoot::Event(root.id));
        assert_eq!(refs.parent, CommentRoot::Event(middle.id));
        assert_eq!(refs.root_kind, Some(1));
        assert_eq!(refs.parent_kind, Some(1));
        assert_eq!(references(&full).root, Some(root.id));
        assert_eq!(references(&full).parent, Some(middle.id));

        // Only `E`: the comment answers its root.
        let lone = comment(&keys, 6, "lone", &[&["E", root_hex.as_str()]]);
        assert_eq!(references(&lone).root, Some(root.id));
        assert_eq!(references(&lone).parent, Some(root.id));

        // An addressable root (`A`: `<kind>:<pubkey>:<d>` per NIP-01 / NIP-22).
        let article_coord = format!("30023:{author_hex}:slug");
        let article = comment(
            &keys,
            7,
            "on an article",
            &[
                &["A", article_coord.as_str(), "wss://relay.example.com"],
                &["K", "30023"],
            ],
        );
        assert_eq!(
            comment_references(&article).root,
            CommentRoot::Address {
                kind: 30023,
                author: keys.public_key(),
                d: "slug".to_string(),
            }
        );
        assert_eq!(
            references(&article).root,
            None,
            "an address is not an event id"
        );

        // `d` identifier containing colons and `a` parent coordinate both parse cleanly.
        let colon_coord = format!("30023:{author_hex}:part1:part2");
        let nested_on_article = comment(
            &keys,
            7,
            "colon d",
            &[&["A", colon_coord.as_str()], &["a", colon_coord.as_str()]],
        );
        assert_eq!(
            comment_references(&nested_on_article).parent,
            CommentRoot::Address {
                kind: 30023,
                author: keys.public_key(),
                d: "part1:part2".to_string(),
            }
        );

        // A comment on an author (`I`), not an event.
        let on_author = comment(&keys, 8, "on an author", &[&["I", author_hex.as_str()]]);
        assert_eq!(comment_references(&on_author).root, CommentRoot::Author);

        // A kind 1 note's NIP-10 reading is unchanged.
        let reply = note(&keys, 9, "reply", &[&["e", root_hex.as_str(), "", "reply"]]);
        assert_eq!(references(&reply).parent, Some(root.id));
    }

    /// expect: when the root itself was not found, it is missing and its
    /// answers are shown as notes with a missing parent.
    #[test]
    fn a_missing_root_is_named_as_missing() {
        let keys = Keys::generate();
        let root = note(&keys, 1, "root", &[]);
        let answer = note(&keys, 2, "answer", &[&["e", &root.id.to_hex(), "", "root"]]);
        let events = [answer];

        let assembled = assemble(root.id, &events);
        assert_eq!(texts(&assembled), [("answer", 0)]);
        assert_eq!(assembled.missing, [root.id]);
        assert!(assembled.entries[0].parent_missing);
    }
}
