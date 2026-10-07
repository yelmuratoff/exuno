//! `exuno generate`: `cmd_generate` of `lib/helpers/generate.sh`, which
//! prints the shipped prompt with an optional project description, and asks
//! for one on a terminal.

use std::io::Write;

use super::put;
use crate::Error;
use crate::output::help::{Help, Section};
use crate::output::style::Style;

/// `lib/prompts/generate.md`, embedded.
pub const PROMPT: &str = include_str!("../../lib/prompts/generate.md");

pub const HELP: Help = Help {
    command: "generate",
    tagline: "print an AI prompt that generates project-specific config",
    synopsis: &["generate [<project description>...]"],
    description: &[
        "Prints an AI prompt that generates project-specific rules, skills,\ncommands, and agents for .ai/src/, and copies it to the clipboard when\none is available.",
        "Words after the command become the project description at the top of the\nprompt. In a terminal with no description, a short menu asks for one.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[("-h, --help", "Show this help")],
    }],
    examples: &[
        "generate",
        "generate React + TypeScript + Next.js project with Prisma ORM",
        "generate > prompt.md",
    ],
};

/// What `generate` takes from the process.
pub struct Env<'a> {
    /// `-t 0` and `-t 1`: the menu opens only when both are terminals.
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    /// The first of `pbcopy`, `wl-copy`, `xclip -selection clipboard`,
    /// `xsel --clipboard --input` on `PATH`, for the tip.
    pub clipboard: Option<String>,
    /// `read -r` on stdin: the line without its newline, `None` at end of input.
    pub read_line: &'a mut dyn FnMut() -> Option<String>,
}

/// `_output_prompt`.
fn output_prompt(
    context: &str,
    style: &Style,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Error> {
    let mut text = String::new();
    if !context.is_empty() {
        text.push_str("## My Project\n\n");
        text.push_str(context);
        text.push_str("\n\n---\n\n");
    }
    text.push_str(PROMPT.trim_end_matches('\n'));
    text.push('\n');
    if !env.stdout_tty {
        return put(out, text.as_bytes());
    }
    put(
        err,
        format!(
            "\n  {}\n\n",
            style.dim("─── prompt below ───────────────────────────────────────────")
        )
        .as_bytes(),
    )?;
    put(out, text.as_bytes())?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    put(
        err,
        format!(
            "\n  {}\n\n",
            style.dim("─── end of prompt ──────────────────────────────────────────")
        )
        .as_bytes(),
    )?;
    if let Some(clipboard) = &env.clipboard {
        put(
            err,
            format!(
                "  {} {} {}\n\n",
                style.dim("Tip: run"),
                style.cyan(&format!("exuno generate | {clipboard}")),
                style.dim("to copy to clipboard.")
            )
            .as_bytes(),
        )?;
    }
    Ok(())
}

/// `cmd_generate`: the prompt on `out`, the conversation on `err`. Input that
/// ends at the menu cancels the run; in the description it ends the text.
pub fn generate(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        put(out, HELP.render(style).as_bytes())?;
        return Ok(0);
    }
    let context = args.join(" ");
    if !context.is_empty() {
        output_prompt(&context, style, env, out, err)?;
        return Ok(0);
    }
    if !env.stdin_tty || !env.stdout_tty {
        output_prompt("", style, env, out, err)?;
        return Ok(0);
    }
    put(
        err,
        format!(
            "\n{}\n\n  Choose what to generate:\n\n    {} Base prompt only\n       Ready-to-paste prompt without project details.\n\n    {} Prompt + project description\n       You describe your stack, and it gets included in the prompt.\n\n",
            style.bold("  Exuno Generate"),
            style.cyan("1)"),
            style.cyan("2)")
        )
        .as_bytes(),
    )?;
    let mut choice = String::new();
    while choice != "1" && choice != "2" {
        put(
            err,
            format!("  {} Choice [1/2]: ", style.green("▸")).as_bytes(),
        )?;
        err.flush().map_err(|e| Error::io("<stderr>", e))?;
        let Some(line) = (env.read_line)() else {
            put(err, b"\nCancelled.\n")?;
            return Ok(1);
        };
        choice = if line.is_empty() {
            "1".to_string()
        } else {
            line
        };
    }
    if choice == "1" {
        put(err, b"\n")?;
        output_prompt("", style, env, out, err)?;
        return Ok(0);
    }
    put(
        err,
        format!(
            "\n  ╭─────────────────────────────────────────────────────────╮\n  │  Describe your project: stack, frameworks, conventions  │\n  │  Type as many lines as you want.                        │\n  │  Press {} twice (empty line) when done.              │\n  ╰─────────────────────────────────────────────────────────╯\n\n",
            style.cyan("Enter")
        )
        .as_bytes(),
    )?;
    let mut lines = String::new();
    let mut prev_empty = false;
    loop {
        put(err, format!("  {} ", style.dim("│")).as_bytes())?;
        err.flush().map_err(|e| Error::io("<stderr>", e))?;
        let Some(line) = (env.read_line)() else {
            break;
        };
        if line.is_empty() {
            if prev_empty {
                break;
            }
            prev_empty = true;
            lines.push('\n');
        } else {
            prev_empty = false;
            if !lines.is_empty() {
                lines.push('\n');
            }
            lines.push_str(&line);
        }
    }
    let lines = lines.trim_end_matches('\n');
    put(err, b"\n")?;
    output_prompt(lines, style, env, out, err)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        args: &[&str],
        stdin_tty: bool,
        stdout_tty: bool,
        clipboard: Option<&str>,
        lines: &[&str],
    ) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut queue: std::collections::VecDeque<String> =
            lines.iter().map(|l| l.to_string()).collect();
        let mut read_line = move || queue.pop_front();
        let mut env = Env {
            stdin_tty,
            stdout_tty,
            clipboard: clipboard.map(str::to_string),
            read_line: &mut read_line,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = generate(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn prompt() -> String {
        format!("{}\n", PROMPT.trim_end_matches('\n'))
    }

    #[test]
    fn a_piped_run_prints_the_raw_prompt_like_cmd_generate() {
        let (status, out, err) = run(&[], false, false, Some("pbcopy"), &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(out, prompt());
        let (status, out, err) = run(&["Flutter app", "with BLoC"], true, false, None, &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "## My Project\n\nFlutter app with BLoC\n\n---\n\n{}",
                prompt()
            )
        );
    }

    #[test]
    fn help_renders_the_shared_shape() {
        let (status, out, err) = run(&["--help"], true, true, Some("pbcopy"), &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\n  exuno generate — print an AI prompt that generates project-specific config\n\n  USAGE\n    exuno generate [<project description>...]\n\n  DESCRIPTION\n    Prints an AI prompt that generates project-specific rules, skills,\n    commands, and agents for .ai/src/, and copies it to the clipboard when\n    one is available.\n\n    Words after the command become the project description at the top of the\n    prompt. In a terminal with no description, a short menu asks for one.\n\n  OPTIONS\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno generate\n    exuno generate React + TypeScript + Next.js project with Prisma ORM\n    exuno generate > prompt.md\n\n"
        );
    }

    #[test]
    fn a_terminal_gets_the_decorations_and_the_clipboard_tip() {
        let (status, out, err) = run(&["React"], false, true, Some("pbcopy"), &[]);
        assert_eq!(status, 0);
        assert!(out.starts_with("## My Project\n\nReact\n\n---\n\n"));
        assert_eq!(
            err,
            "\n  ─── prompt below ───────────────────────────────────────────\n\n\n  ─── end of prompt ──────────────────────────────────────────\n\n  Tip: run exuno generate | pbcopy to copy to clipboard.\n\n"
        );
        let (_, _, err) = run(&["React"], false, true, None, &[]);
        assert!(!err.contains("Tip: run"));
    }

    #[test]
    fn the_menu_reads_a_choice_and_a_description_like_cmd_generate() {
        let (status, out, err) = run(&[], true, true, None, &[""]);
        assert_eq!(status, 0);
        assert_eq!(out, prompt());
        assert!(err.starts_with(
            "\n  Exuno Generate\n\n  Choose what to generate:\n\n    1) Base prompt only\n"
        ));
        assert!(err.contains("  ▸ Choice [1/2]: \n\n  ─── prompt below"));
        let (status, out, err) = run(
            &[],
            true,
            true,
            None,
            &["x", "2", "Rust CLI", "", "with clap", "", ""],
        );
        assert_eq!(status, 0);
        assert!(out.starts_with("## My Project\n\nRust CLI\n\nwith clap\n\n---\n\n"));
        assert_eq!(err.matches("Choice [1/2]:").count(), 2);
        assert!(err.contains("  ╭─────"));
        assert!(err.contains("  │   │   │   │   │ \n\n  ─── prompt below"));
        let (status, out, _) = run(&[], true, true, None, &["2", "only line"]);
        assert_eq!(status, 0);
        assert!(out.starts_with("## My Project\n\nonly line\n\n---\n\n"));
        let (status, out, err) = run(&[], true, true, None, &[]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert!(err.ends_with("Choice [1/2]: \nCancelled.\n"), "{err}");
    }
}
