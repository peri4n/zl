use std::{fs, str::FromStr};

use chrono::{DateTime, Local};

use crate::notes::Note;

/// Output format of a listed note, parsed from a string like `{path}\t{title}`
#[derive(Debug, Clone)]
pub(crate) struct Format(Vec<Segment>);

#[derive(Debug, Clone)]
enum Segment {
    Literal(String),
    Placeholder(Placeholder),
}

#[derive(Debug, Clone, Copy)]
enum Placeholder {
    Path,
    Dir,
    Filename,
    Title,
    Tags,
    MdLink,
    Modified,
}

impl Placeholder {
    const ALL: [(&str, Placeholder); 7] = [
        ("path", Placeholder::Path),
        ("dir", Placeholder::Dir),
        ("filename", Placeholder::Filename),
        ("title", Placeholder::Title),
        ("tags", Placeholder::Tags),
        ("mdlink", Placeholder::MdLink),
        ("modified", Placeholder::Modified),
    ];

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
    }

    fn render(self, note: &Note) -> String {
        match self {
            Placeholder::Path => note.path.display().to_string(),
            Placeholder::Dir => note.path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
            Placeholder::Filename => note.path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
            Placeholder::Title => note.title.clone(),
            Placeholder::Tags => note.tags.join(", "),
            Placeholder::MdLink => {
                let path = note.path.display().to_string();
                // Markdown link destinations can't contain spaces unless wrapped in angle brackets
                if path.contains(' ') {
                    format!("[{}](<{}>)", note.title, path)
                } else {
                    format!("[{}]({})", note.title, path)
                }
            }
            Placeholder::Modified => fs::metadata(&note.path)
                .and_then(|metadata| metadata.modified())
                .map(|time| DateTime::<Local>::from(time).format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
        }
    }
}

impl Format {
    pub(crate) fn render(&self, note: &Note) -> String {
        self.0.iter()
            .map(|segment| match segment {
                Segment::Literal(text) => text.clone(),
                Segment::Placeholder(placeholder) => placeholder.render(note),
            })
            .collect()
    }
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut rest = s;

        while let Some(c) = rest.chars().next() {
            rest = &rest[c.len_utf8()..];

            match c {
                '{' if rest.starts_with('{') => {
                    rest = &rest[1..];
                    literal.push('{');
                }
                '{' => {
                    let end = rest.find('}')
                        .ok_or("unclosed `{` in format, use `{{` for a literal brace")?;
                    let name = &rest[..end];
                    let placeholder = Placeholder::from_name(name).ok_or_else(|| {
                        let names = Placeholder::ALL.iter().map(|(n, _)| format!("{{{}}}", n)).collect::<Vec<_>>();
                        format!("unknown placeholder `{{{}}}`, expected one of: {}", name, names.join(", "))
                    })?;

                    if !literal.is_empty() {
                        segments.push(Segment::Literal(std::mem::take(&mut literal)));
                    }
                    segments.push(Segment::Placeholder(placeholder));
                    rest = &rest[end + 1..];
                }
                '}' if rest.starts_with('}') => {
                    rest = &rest[1..];
                    literal.push('}');
                }
                '}' => return Err("unmatched `}` in format, use `}}` for a literal brace".to_string()),
                // Most shells pass `\t` literally, so support the common escapes for TSV like output
                '\\' if rest.starts_with(['t', 'n', '\\']) => {
                    literal.push(match &rest[..1] {
                        "t" => '\t',
                        "n" => '\n',
                        _ => '\\',
                    });
                    rest = &rest[1..];
                }
                c => literal.push(c),
            }
        }

        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }

        Ok(Format(segments))
    }
}
