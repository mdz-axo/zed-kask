//! Putting a conversation in order (NIP-10).
//!
//! A reply names its conversation's root and the note it answers in `e`
//! tags. Nothing here contacts a relay: given the events that were found,
//! this module works out who answers whom, and which notes the conversation
//! refers to that were not found.
//!
//! Scope: kind 1 notes and their kind 1 replies. NIP-22 comments (kind
//! 1111) name their root differently and are not assembled here.

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

/// Read a note's `e` tags the way NIP-10 says.
///
/// Marked tags win: `root` names the root and `reply` the parent; a note
/// with only a `root` answers the root. Without any marker the older
/// positional rule applies: the first `e` tag is the root and the last is
/// the parent. A tag marked `mention` is a citation, not a parent.
pub fn references(event: &Event) -> References {
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
