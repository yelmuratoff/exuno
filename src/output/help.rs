//! One shape for every command's `--help`, matching the top-level usage:
//! a bold `exuno <command> — <tagline>` line, green section headings,
//! and option columns aligned on the longest term. Plain when `Style` is.

use super::style::Style;

/// A titled block of `term  description` rows; a description holds its own
/// continuation lines separated by `\n`, each aligned under the first.
pub struct Section {
    pub title: &'static str,
    pub entries: &'static [(&'static str, &'static str)],
}

pub struct Help {
    pub command: &'static str,
    pub tagline: &'static str,
    /// Synopsis lines without the `exuno` prefix, e.g. `sync [OPTIONS]`.
    pub synopsis: &'static [&'static str],
    /// Paragraphs under DESCRIPTION, each already wrapped; blank between.
    pub description: &'static [&'static str],
    pub sections: &'static [Section],
    /// Example invocations without the `exuno` prefix, with an optional
    /// `  # comment` tail.
    pub examples: &'static [&'static str],
}

const INDENT: &str = "    ";

impl Help {
    pub fn render(&self, style: &Style) -> String {
        let mut text = format!(
            "\n  {} — {}\n\n  {}\n",
            style.bold(&format!("exuno {}", self.command)),
            self.tagline,
            style.green("USAGE")
        );
        for line in self.synopsis {
            text.push_str(&format!("{INDENT}exuno {line}\n"));
        }
        if !self.description.is_empty() {
            text.push_str(&format!("\n  {}\n", style.green("DESCRIPTION")));
            for (i, paragraph) in self.description.iter().enumerate() {
                if i > 0 {
                    text.push('\n');
                }
                for line in paragraph.lines() {
                    text.push_str(&format!("{INDENT}{line}\n"));
                }
            }
        }
        for section in self.sections {
            text.push_str(&format!("\n  {}\n", style.green(section.title)));
            let longest = section
                .entries
                .iter()
                .map(|(term, _)| term.chars().count())
                .max()
                .unwrap_or(0);
            let width = if longest == 0 { 0 } else { longest + 3 };
            for (term, description) in section.entries {
                let mut lines = description.lines();
                let first = lines.next().unwrap_or("");
                text.push_str(&format!("{INDENT}{}{first}\n", pad(term, width)));
                for line in lines {
                    text.push_str(&format!("{INDENT}{}{line}\n", " ".repeat(width)));
                }
            }
        }
        if !self.examples.is_empty() {
            text.push_str(&format!("\n  {}\n", style.green("EXAMPLES")));
            for example in self.examples {
                text.push_str(&format!("{INDENT}exuno {example}\n"));
            }
        }
        text.push('\n');
        text
    }

    /// The first synopsis line with its prefix, for a `Error: …` refusal.
    pub fn synopsis_line(&self) -> String {
        format!("exuno {}", self.synopsis.first().unwrap_or(&self.command))
    }
}

fn pad(term: &str, width: usize) -> String {
    let len = term.chars().count();
    format!("{term}{}", " ".repeat(width.saturating_sub(len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP: Help = Help {
        command: "demo",
        tagline: "show the shape",
        synopsis: &["demo [<name>] [OPTIONS]", "demo --all"],
        description: &["First paragraph\nwith two lines.", "Second paragraph."],
        sections: &[Section {
            title: "OPTIONS",
            entries: &[
                ("--all", "Everything"),
                ("-y, --yes", "Skip the prompt\nand the second line"),
            ],
        }],
        examples: &["demo", "demo --all   # comment"],
    };

    #[test]
    fn a_plain_render_has_the_shape_of_the_top_level_usage() {
        assert_eq!(
            HELP.render(&Style::plain()),
            "\n  exuno demo — show the shape\n\n  USAGE\n    exuno demo [<name>] [OPTIONS]\n    exuno demo --all\n\n  DESCRIPTION\n    First paragraph\n    with two lines.\n\n    Second paragraph.\n\n  OPTIONS\n    --all       Everything\n    -y, --yes   Skip the prompt\n                and the second line\n\n  EXAMPLES\n    exuno demo\n    exuno demo --all   # comment\n\n"
        );
        assert_eq!(HELP.synopsis_line(), "exuno demo [<name>] [OPTIONS]");
    }

    #[test]
    fn colour_touches_only_the_title_and_the_headings() {
        let text = HELP.render(&Style::colored());
        assert!(text.starts_with(
            "\n  \x1b[1mexuno demo\x1b[0m — show the shape\n\n  \x1b[32mUSAGE\x1b[0m\n"
        ));
        assert!(text.contains("\n  \x1b[32mOPTIONS\x1b[0m\n    --all       Everything\n"));
    }

    #[test]
    fn empty_parts_are_left_out() {
        let help = Help {
            command: "version",
            tagline: "print the version",
            synopsis: &["version"],
            description: &[],
            sections: &[],
            examples: &[],
        };
        assert_eq!(
            help.render(&Style::plain()),
            "\n  exuno version — print the version\n\n  USAGE\n    exuno version\n\n"
        );
    }
}
