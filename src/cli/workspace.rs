//! `exuno sync --workspace`: `cmd_workspace_fanout` of `bin/agentsync.sh`,
//! one sync per project below the working directory, deepest first.

use crate::cli::sync::{self, Env};
use crate::output::log::{Sink, Stream};
use crate::output::style::Style;
use crate::paths;

/// Syncs every project below `cwd` with `args`, `--workspace` removed. The
/// status is that of the last project that failed, or 0.
pub fn run(
    cwd: &str,
    args: &[String],
    env: &Env,
    style: &Style,
    colors: bool,
    streams: &dyn Fn() -> Sink,
) -> u8 {
    let mut emit = streams();
    let projects = paths::find_workspace_ai_dirs(cwd);
    if projects.is_empty() {
        emit(
            Stream::Err,
            &format!(
                "{}: No .ai/ directories found below {cwd}",
                style.red("Error")
            ),
        );
        emit(
            Stream::Err,
            &format!(
                "Run {} to create one, or run from a workspace root.",
                style.cyan("exuno init")
            ),
        );
        return 1;
    }

    emit(Stream::Err, "");
    emit(Stream::Err, &style.bold("  Exuno workspace sync"));
    emit(
        Stream::Err,
        &style.dim(&format!(
            "  Found {} project(s) below {cwd}",
            projects.len()
        )),
    );
    emit(Stream::Err, "");

    let mut max_status = 0;
    for ai in &projects {
        let root = paths::parent(ai);
        let rel = if root == cwd {
            ".".to_string()
        } else {
            root.strip_prefix(&format!("{cwd}/"))
                .unwrap_or(&root)
                .to_string()
        };
        emit(Stream::Err, &format!("  {} {rel}", style.cyan("→")));
        let status = sync::run(&root, args, env, colors, streams());
        max_status = max_status.max(status);
        emit(Stream::Err, "");
    }

    if max_status == 0 {
        emit(
            Stream::Err,
            &format!(
                "  {} {} project(s) processed.",
                style.green("Workspace sync complete."),
                projects.len()
            ),
        );
    } else {
        emit(
            Stream::Err,
            &format!(
                "  {} max exit code: {max_status}",
                style.yellow("Workspace sync finished with errors.")
            ),
        );
    }
    emit(Stream::Err, "");
    max_status
}
