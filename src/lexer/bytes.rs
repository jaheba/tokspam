//! Byte classification and bounded searches; no literal decoding or Unicode normalization.

pub(super) const NAME_BYTE: u8 = 1;
pub(super) const NUMBER_BYTE: u8 = 2;

// One lookup classifies continuation bytes in the hot identifier/number loops.
// Non-ASCII bytes are kept together as names, matching the streaming lexer.
pub(super) const BYTE_CLASSES: [u8; 256] = {
    let mut classes = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let ascii_word = (byte >= b'a' as usize && byte <= b'z' as usize)
            || (byte >= b'A' as usize && byte <= b'Z' as usize)
            || (byte >= b'0' as usize && byte <= b'9' as usize)
            || byte == b'_' as usize;
        if ascii_word || byte >= 128 {
            classes[byte] |= NAME_BYTE;
        }
        if ascii_word || byte == b'.' as usize {
            classes[byte] |= NUMBER_BYTE;
        }
        byte += 1;
    }
    classes
};

// Keep nearby quotes and escapes cheap before using memchr's vectorized search.
// Delimiters are ASCII, so matches remain UTF-8 boundaries even when a search
// reads across multibyte characters.
#[inline(always)]
pub(super) fn find_ascii<const N: usize>(text: &[u8], needles: [u8; N]) -> usize {
    let prefix = text.len().min(8);
    if let Some(offset) = text[..prefix]
        .iter()
        .position(|byte| needles.contains(byte))
    {
        return offset;
    }
    let tail = &text[prefix..];
    let found = match needles.as_slice() {
        [a, b] => memchr::memchr2(*a, *b, tail),
        [a, b, c] => memchr::memchr3(*a, *b, *c, tail),
        [a, b, c, d] => {
            // memchr searches up to three bytes. Bound both searches to a
            // constant-size block: repeated fourth-byte hits must not cause
            // rescanning of the entire remaining literal on every escape.
            let mut offset = 0;
            for chunk in tail.chunks(64) {
                let first = memchr::memchr3(*a, *b, *c, chunk).unwrap_or(chunk.len());
                if let Some(last) = memchr::memchr(*d, &chunk[..first]) {
                    return prefix + offset + last;
                }
                if first < chunk.len() {
                    return prefix + offset + first;
                }
                offset += chunk.len();
            }
            None
        }
        _ => unreachable!("literal searches use two, three, or four delimiters"),
    };
    prefix + found.unwrap_or(tail.len())
}
