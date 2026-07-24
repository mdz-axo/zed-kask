//! UTF-8-safe operations for input cursors stored as byte offsets.

pub(crate) fn insert(text: &mut String, cursor: &mut usize, ch: char) {
    debug_assert!(text.is_char_boundary(*cursor));
    text.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

pub(crate) fn backspace(text: &mut String, cursor: &mut usize) -> bool {
    let Some(previous) = previous_boundary(text, *cursor) else {
        return false;
    };
    text.drain(previous..*cursor);
    *cursor = previous;
    true
}

pub(crate) fn delete(text: &mut String, cursor: usize) -> bool {
    let Some(ch) = text.get(cursor..).and_then(|rest| rest.chars().next()) else {
        return false;
    };
    text.drain(cursor..cursor + ch.len_utf8());
    true
}

pub(crate) fn move_left(text: &str, cursor: &mut usize) -> bool {
    let Some(previous) = previous_boundary(text, *cursor) else {
        return false;
    };
    *cursor = previous;
    true
}

pub(crate) fn move_right(text: &str, cursor: &mut usize) -> bool {
    let Some(ch) = text.get(*cursor..).and_then(|rest| rest.chars().next()) else {
        return false;
    };
    *cursor += ch.len_utf8();
    true
}

pub(crate) fn parts(text: &str, cursor: usize) -> (&str, Option<char>, &str) {
    debug_assert!(text.is_char_boundary(cursor));
    let before = &text[..cursor];
    let Some(ch) = text[cursor..].chars().next() else {
        return (before, None, "");
    };
    let after = &text[cursor + ch.len_utf8()..];
    (before, Some(ch), after)
}

fn previous_boundary(text: &str, cursor: usize) -> Option<usize> {
    debug_assert!(text.is_char_boundary(cursor));
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_multibyte_text_at_character_boundaries() {
        let mut text = String::new();
        let mut cursor = 0;
        insert(&mut text, &mut cursor, 'é');
        insert(&mut text, &mut cursor, '界');
        assert_eq!(text, "é界");
        assert_eq!(cursor, text.len());

        assert!(move_left(&text, &mut cursor));
        assert_eq!(parts(&text, cursor), ("é", Some('界'), ""));
        assert!(delete(&mut text, cursor));
        assert_eq!(text, "é");
        assert!(backspace(&mut text, &mut cursor));
        assert!(text.is_empty());
        assert_eq!(cursor, 0);
    }
}
