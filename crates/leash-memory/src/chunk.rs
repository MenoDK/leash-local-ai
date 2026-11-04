//! Chunking that respects Markdown structure.
//!
//! A chunk is the text under one heading, with the path of headings above it kept as context (`Guide › Setup › Linux`).
//! That is what makes a retrieved passage understandable alone, and gives a citation a place to point to.

/// A passage and where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Headings from the top down, joined with ` › `. Empty for text before the first heading.
    pub heading_path: String,
    pub text: String,
}

fn heading_level(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
        Some((hashes, trimmed[hashes..].trim()))
    } else {
        None
    }
}

/// Split `text` into pieces of at most `max_chars` characters, on paragraph boundaries where possible and anywhere
/// (never inside a character) when one paragraph is longer than the limit.
fn split(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_owned()];
    }
    fn flush(pieces: &mut Vec<String>, current: &mut String) {
        if !current.trim().is_empty() {
            pieces.push(current.trim().to_owned());
        }
        current.clear();
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        if paragraph.chars().count() > max_chars {
            flush(&mut pieces, &mut current);
            let chars: Vec<char> = paragraph.chars().collect();
            for part in chars.chunks(max_chars) {
                let piece: String = part.iter().collect();
                if !piece.trim().is_empty() {
                    pieces.push(piece.trim().to_owned());
                }
            }
            continue;
        }
        if current.chars().count() + paragraph.chars().count() + 2 > max_chars {
            flush(&mut pieces, &mut current);
        }
        current.push_str(paragraph);
        current.push_str("\n\n");
    }
    flush(&mut pieces, &mut current);
    pieces
}

/// Chunk a Markdown document. A `#` inside a fenced code block is code, not a heading.
#[must_use]
pub fn chunk_markdown(markdown: &str, max_chars: usize) -> Vec<Chunk> {
    let max_chars = max_chars.max(80);
    let mut chunks = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut body = String::new();
    let mut in_fence = false;

    let emit = |chunks: &mut Vec<Chunk>, stack: &[(usize, String)], body: &str| {
        let text = body.trim();
        if text.is_empty() {
            return;
        }
        let heading_path = stack
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" › ");
        for piece in split(text, max_chars) {
            chunks.push(Chunk {
                heading_path: heading_path.clone(),
                text: piece,
            });
        }
    };

    for line in markdown.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
        }
        match (in_fence, heading_level(line)) {
            (false, Some((level, title))) => {
                emit(&mut chunks, &stack, &body);
                body.clear();
                while stack.last().is_some_and(|(l, _)| *l >= level) {
                    stack.pop();
                }
                stack.push((level, title.to_owned()));
            }
            _ => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    emit(&mut chunks, &stack, &body);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_path_of_headings_as_context() {
        let md = "intro\n# Guide\nwelcome\n## Setup\nsteps\n### Linux\nuse apt\n## Usage\nrun it\n";
        let chunks = chunk_markdown(md, 1000);
        let paths: Vec<(&str, &str)> = chunks
            .iter()
            .map(|c| (c.heading_path.as_str(), c.text.as_str()))
            .collect();
        assert_eq!(
            paths,
            [
                ("", "intro"),
                ("Guide", "welcome"),
                ("Guide › Setup", "steps"),
                ("Guide › Setup › Linux", "use apt"),
                ("Guide › Usage", "run it")
            ]
        );
    }

    #[test]
    fn a_hash_inside_a_code_fence_is_not_a_heading() {
        let md = "# Title\n```sh\n# install it\nmake\n```\nafter\n";
        let chunks = chunk_markdown(md, 1000);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("# install it"));
    }

    #[test]
    fn a_long_section_is_split_without_breaking_a_character() {
        let paragraph = "é".repeat(500);
        let md = format!(
            "# T\n{paragraph}\n\n{paragraph}\n\n{}",
            "日本語".repeat(300)
        );
        let chunks = chunk_markdown(&md, 300);
        assert!(chunks.len() >= 4);
        assert!(
            chunks.iter().all(|c| c.text.chars().count() <= 300),
            "a piece is over the limit"
        );
        assert!(chunks.iter().all(|c| c.heading_path == "T"));
    }

    #[test]
    fn empty_and_heading_only_documents_give_nothing() {
        assert!(chunk_markdown("", 500).is_empty());
        assert!(chunk_markdown("# only a title\n\n## and another\n", 500).is_empty());
    }

    #[test]
    fn a_hash_without_a_space_is_text() {
        let chunks = chunk_markdown("#hashtag and #1\n", 500);
        assert_eq!(chunks[0].text, "#hashtag and #1");
    }
}
