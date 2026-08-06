/// Split text into ~1000-char segments with ~20-char overlap.
pub fn chunk_text(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![];
    }
    let size = size.max(1);
    let overlap = overlap.min(size.saturating_sub(1));
    let step = (size - overlap).max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let end = (i + size).min(chars.len());
        out.push(chars[i..end].iter().collect());
        if end >= chars.len() {
            break;
        }
        i += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlaps_adjacent_chunks() {
        let s: String = (0..1200).map(|_| '字').collect();
        let chunks = chunk_text(&s, 1000, 20);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 1000);
        assert_eq!(chunks[1].chars().count(), 220);
        let a: String = chunks[0].chars().skip(980).collect();
        let b: String = chunks[1].chars().take(20).collect();
        assert_eq!(a, b);
    }
}
