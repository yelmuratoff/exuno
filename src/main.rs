use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use agentsync::cli::{self, Command};
use agentsync::config::names;
use agentsync::engine::render::Env;
use agentsync::output::log::{Sink, Stream};
use agentsync::output::style::Style;
use agentsync::project::Project;
use agentsync::{Error, engine_version, output::prompts, paths};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(args) {
        Ok(status) => ExitCode::from(status),
        Err(e) if e.is_broken_pipe() => ExitCode::SUCCESS,
        Err(e) => {
            // Bash decides colour from stdout even for stderr lines; same here.
            eprintln!("{}: {e}", Style::for_stdout().red("Error"));
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<u8, Error> {
    let first = args.first().and_then(|a| a.to_str()).unwrap_or("");
    if cli::notice::wants_notice(first) {
        check_for_updates()?;
    }
    let words: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if cli::usage::wants_usage(&words) {
        return print_usage();
    }
    let Some(command) = Command::parse(first) else {
        let word = words.first().map(String::as_str).unwrap_or_default();
        return cli::usage::unknown_command(word, &Style::for_stdout(), &mut std::io::stderr());
    };
    if matches!(command, Command::UpdateCache) {
        if let Some(cache) = args.get(1) {
            cli::notice::refresh_cache(Path::new(cache));
        }
        return Ok(0);
    }
    dispatch(command, &words[1..], &Style::for_stdout())
}

type Discovering = fn(
    &[String],
    &dyn Fn() -> Result<Project, Error>,
    &Style,
    &mut dyn Write,
    &mut dyn Write,
) -> Result<u8, Error>;

type Rooted = fn(&[String], &str, &Style, &mut dyn Write, &mut dyn Write) -> Result<u8, Error>;

/// A command that finds its project itself, writing to the process streams.
fn discovering(command: Discovering, rest: &[String], style: &Style) -> Result<u8, Error> {
    command(
        rest,
        &Project::discover,
        style,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// A command given the project root, writing to the process streams.
fn rooted(command: Rooted, rest: &[String], root: &str, style: &Style) -> Result<u8, Error> {
    command(
        rest,
        root,
        style,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn dispatch(command: Command, rest: &[String], style: &Style) -> Result<u8, Error> {
    let (stdout, stderr) = (&mut std::io::stdout(), &mut std::io::stderr());
    match command {
        Command::Version => print_version(),
        Command::Skills => {
            let env = sync_env();
            cli::skills::run(rest, &repo_root()?, &env.render, style, stdout, stderr)
        }
        Command::Mcp => cli::mcp::run(rest, style, stdout, stderr),
        Command::Catalog => write_stdout(&cli::update::catalog_dump()),
        Command::UpdateCache => Ok(0),
        Command::Update => update_command(rest, style),
        Command::Dedupe => {
            let mut read = prompts::read_terminal;
            let answer = prompts::is_tty().then_some(&mut read as &mut dyn FnMut() -> String);
            let cwd = logical_cwd()?;
            let config = path_setting("CONFIG_PATH");
            let place = cli::dedupe::Place {
                cwd: &cwd,
                root: &project_root,
                config: config.as_deref(),
            };
            cli::dedupe::dedupe(rest, &place, style, answer, stdout, stderr)
        }
        Command::Migrate => migrate_command(rest, style),
        Command::Generate => generate_command(rest, style),
        Command::ShellInit => {
            let shell = var("SHELL");
            cli::shell_init::shell_init(rest, shell.as_deref(), style, log_colors(), stdout, stderr)
        }
        Command::SetupHooks => rooted(
            cli::setup_hooks::setup_hooks,
            rest,
            &supplied_root()?,
            style,
        ),
        Command::Release => release_command(rest, style),
        Command::Export => rooted(cli::bundle::export, rest, &repo_root()?, style),
        Command::Import => import_command(rest, style),
        Command::Add => rooted(cli::add::add, rest, &repo_root()?, style),
        Command::Doctor => {
            let env = cli::doctor::Env {
                version: engine_version(),
                external_roots: external_roots_var(),
            };
            cli::doctor::doctor(rest, &Project::discover, style, &env, stdout, stderr)
        }
        Command::Init => init_command(rest, style),
        Command::Refresh => {
            let config = path_setting("CONFIG_PATH");
            let mut env = cli::refresh::Env {
                interactive: prompts::is_tty(),
                read_line: &mut prompts::read_terminal,
                config_path: config.as_deref(),
            };
            cli::refresh::refresh(rest, &supplied_root()?, style, &mut env, stdout, stderr)
        }
        Command::UpgradeConfig => cli::upgrade_config::run(
            rest,
            &Project::discover,
            engine_version(),
            style,
            stdout,
            stderr,
        ),
        Command::Enable => confirming(cli::enable::enable, rest, style, true),
        Command::Disable => discovering(cli::enable::disable, rest, style),
        Command::Show => discovering(cli::show::show, rest, style),
        Command::Diff => discovering(cli::diff::diff, rest, style),
        Command::Adopt => confirming(cli::adopt::adopt, rest, style, false),
        Command::Profile => confirming(cli::profile::profile, rest, style, false),
        Command::Resolve => asking(cli::resolve::resolve, rest, style, "        "),
        Command::Simplify => asking(cli::simplify::simplify, rest, style, "  "),
        Command::Customize => customize_command(rest, style),
        Command::List => cli::list::run(rest, &Project::discover, style, &mut stdout.lock()),
        Command::Check => check_command(rest, style),
        Command::Sync => sync_command(rest, style),
        Command::Rollback => rollback_command(rest, style),
    }
}

type Confirming = fn(
    &[String],
    &dyn Fn() -> Result<Project, Error>,
    &Style,
    bool,
    &mut dyn FnMut(&str) -> bool,
    &mut dyn Write,
    &mut dyn Write,
) -> Result<u8, Error>;

type Asking = fn(
    &[String],
    &dyn Fn() -> Result<Project, Error>,
    &Style,
    bool,
    &mut dyn FnMut(&str, &mut dyn Write) -> String,
    &mut dyn Write,
    &mut dyn Write,
) -> Result<u8, Error>;

/// A discovering command that asks yes/no on the terminal, `default_yes`
/// answering an empty reply.
fn confirming(
    command: Confirming,
    rest: &[String],
    style: &Style,
    default_yes: bool,
) -> Result<u8, Error> {
    command(
        rest,
        &Project::discover,
        style,
        prompts::is_tty(),
        &mut |question: &str| prompts::confirm(question, default_yes),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// A discovering command that reads free answers, each prompt indented by `indent`.
fn asking(command: Asking, rest: &[String], style: &Style, indent: &str) -> Result<u8, Error> {
    command(
        rest,
        &Project::discover,
        style,
        prompts::is_tty(),
        &mut |prompt: &str, out: &mut dyn Write| {
            let _ = write!(out, "{indent}{prompt} ");
            let _ = out.flush();
            prompts::read_terminal()
        },
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn write_stdout(text: &str) -> Result<u8, Error> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())
        .map(|()| 0)
        .map_err(|e| Error::io("<stdout>", e))
}

fn update_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let exe = current_exe().map_err(|e| Error::io("<exe>", e))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut env = cli::update::Env {
        exe,
        project_dir: repo_root()?,
        today: agentsync::config::snapshot::utc_date(now),
        width: cli::update::terminal_width(),
        fetch: &mut cli::update::curl_fetch,
        extract: &mut cli::update::tar_extract,
        ask: &mut cli::update::ask_binary,
    };
    cli::update::update(
        rest,
        style,
        &mut env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn migrate_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let prompt_root = supplied_root()?;
    let path_var = var("PATH");
    let mut env = cli::migrate::Env {
        version: engine_version(),
        prompt_root,
        no_clipboard: setting("NO_CLIPBOARD").as_deref() == Some("1"),
        stdout_tty: std::io::stdout().is_terminal(),
        interactive: prompts::is_tty(),
        confirm: &mut |question: &str, default_yes: bool| prompts::confirm(question, default_yes),
        copy: &mut |text: &str| cli::migrate::copy_to_clipboard(text, path_var.as_deref()),
    };
    cli::migrate::migrate(
        rest,
        &Project::discover,
        style,
        &mut env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn generate_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let mut read_line = || {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    };
    let mut env = cli::generate::Env {
        stdin_tty: std::io::stdin().is_terminal(),
        stdout_tty: std::io::stdout().is_terminal(),
        clipboard: clipboard_command(),
        read_line: &mut read_line,
    };
    cli::generate::generate(
        rest,
        style,
        &mut env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn release_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let mut read_line = || {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches('\n').to_string()),
        }
    };
    let mut env = cli::release::Env {
        cwd: logical_cwd()?,
        install_dir: setting("HOME").filter(|home| Path::new(home).join(".git").is_dir()),
        read_line: &mut read_line,
    };
    cli::release::release(
        rest,
        style,
        &mut env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn import_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let root = repo_root()?;
    let mut read_line = || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        line.trim_end_matches(['\n', '\r']).to_string()
    };
    let mut env = cli::bundle::Env {
        cwd: logical_cwd()?,
        interactive: std::io::stdin().is_terminal(),
        path: var("PATH"),
        read_line: &mut read_line,
    };
    cli::bundle::import(
        rest,
        &root,
        style,
        &mut env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn init_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let cwd = logical_cwd()?;
    let sync_env = sync_env();
    let colors = log_colors();
    let mut sync = |root: &str| cli::sync::run(root, &[], &sync_env, colors, streams());
    let mut confirm = |question: &str, default_yes: bool| prompts::confirm(question, default_yes);
    let mut multiselect = |title: &str, options: &[String], preselected: &[String]| {
        prompts::multiselect_on_terminal(title, options, preselected, style)
    };
    let mut env = cli::init::Env {
        version: engine_version(),
        cwd,
        config_path: path_setting("CONFIG_PATH"),
        backup_limit: setting("BACKUP_LIMIT"),
        backup_max_age: setting("BACKUP_MAX_AGE_DAYS"),
        interactive: prompts::is_tty(),
        confirm: &mut confirm,
        multiselect: &mut multiselect,
        sync: &mut sync,
    };
    cli::init::init(
        rest,
        style,
        &mut env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn customize_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    cli::customize::customize(
        rest,
        &Project::discover,
        style,
        std::io::stdin().is_terminal(),
        &mut |prompt: &str| {
            eprint!("{prompt}");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            line.trim_matches([' ', '\t', '\n']).to_string()
        },
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

fn check_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let root = project_root()?;
    let env = Env {
        config_path: path_setting("CONFIG_PATH"),
        skip_post_sync: Some("true".to_string()),
        allow_post_sync: None,
        backup: None,
        external_source_roots: external_roots_var(),
    };
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    cli::check::run(rest, &root, &env, style, &mut out, &mut err)
}

fn sync_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    if !rest.iter().any(|a| a == "--workspace") {
        let root = project_root()?;
        return Ok(cli::sync::run(
            &root,
            rest,
            &sync_env(),
            log_colors(),
            streams(),
        ));
    }
    let forwarded: Vec<String> = rest
        .iter()
        .filter(|a| *a != "--workspace")
        .cloned()
        .collect();
    let cwd = logical_cwd()?;
    Ok(cli::workspace::run(
        &cwd,
        &forwarded,
        &sync_env(),
        style,
        log_colors(),
        &streams,
    ))
}

fn rollback_command(rest: &[String], style: &Style) -> Result<u8, Error> {
    let supplied_root = supplied_root()?;
    let env = cli::rollback::Env {
        config_path: path_setting("CONFIG_PATH"),
        backup_limit: setting("BACKUP_LIMIT"),
        backup_max_age: setting("BACKUP_MAX_AGE_DAYS"),
    };
    let mut confirm = |question: &str| prompts::confirm(question, false);
    Ok(cli::rollback::run(
        &supplied_root,
        rest,
        &env,
        style,
        &mut confirm,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ))
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// `EXUNO_<suffix>`, else `AGENTSYNC_<suffix>`.
fn setting(suffix: &str) -> Option<String> {
    names::env(suffix, &var)
}

/// A [`setting`] holding one path, translated from Git Bash's spelling on
/// Windows.
fn path_setting(suffix: &str) -> Option<String> {
    setting(suffix).map(|path| paths::from_msys(&path, var("MSYSTEM").as_deref()))
}

/// `AGENTSYNC_EXTERNAL_SOURCE_ROOTS`, colon-separated as Bash reads it; on
/// Windows each entry is translated and the list rejoined with `;`, the
/// separator `Paths::trust_external_roots` splits there.
fn external_roots_var() -> Option<String> {
    let raw = setting("EXTERNAL_SOURCE_ROOTS")?;
    if !cfg!(windows) {
        return Some(raw);
    }
    let msystem = var("MSYSTEM");
    // A drive letter splits into `C` and `/Users/…`; put those back together.
    let mut entries: Vec<String> = Vec::new();
    for piece in raw.split(':') {
        match entries.last_mut() {
            Some(last)
                if last.len() == 1
                    && last.bytes().all(|b| b.is_ascii_alphabetic())
                    && piece.starts_with('/') =>
            {
                last.push(':');
                last.push_str(piece);
            }
            _ => entries.push(piece.to_string()),
        }
    }
    Some(
        entries
            .iter()
            .map(|entry| paths::from_msys(entry, msystem.as_deref()))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

fn sync_env() -> cli::sync::Env {
    let skip_backup = setting("INTERNAL_SKIP_BACKUP").as_deref() == Some("true");
    cli::sync::Env {
        render: Env {
            config_path: path_setting("CONFIG_PATH"),
            skip_post_sync: setting("SKIP_POST_SYNC"),
            allow_post_sync: setting("ALLOW_POST_SYNC"),
            backup: (!skip_backup).then(|| agentsync::engine::render::BackupBounds {
                limit: setting("BACKUP_LIMIT"),
                max_age: setting("BACKUP_MAX_AGE_DAYS"),
            }),
            external_source_roots: external_roots_var(),
        },
        skip_backup,
        backup_limit: setting("BACKUP_LIMIT"),
        backup_max_age: setting("BACKUP_MAX_AGE_DAYS"),
    }
}

/// The clipboard command `_output_prompt` names in its tip: the first of
/// `pbcopy`, `wl-copy`, `xclip`, `xsel` on `PATH`, with the flags Bash printed.
fn clipboard_command() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let on_path = |name: &str| std::env::split_paths(&path).any(|dir| dir.join(name).is_file());
    [
        ("pbcopy", "pbcopy"),
        ("wl-copy", "wl-copy"),
        ("xclip", "xclip -selection clipboard"),
        ("xsel", "xsel --clipboard --input"),
    ]
    .iter()
    .find(|(name, _)| on_path(name))
    .map(|(_, command)| command.to_string())
}

/// `_use_colors` of `logging.sh`, on the stream the log is written to: stderr
/// is a terminal and `NO_COLOR` is empty.
fn log_colors() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal() && var("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Log lines to the process streams as `echo` writes them. A closed stdout does
/// not stop the run: the transaction finishes, as it would with nobody reading.
fn streams() -> Sink {
    Box::new(|stream, line| {
        let _ = match stream {
            Stream::Out => writeln!(std::io::stdout(), "{line}"),
            Stream::Err => writeln!(std::io::stderr(), "{line}"),
        };
    })
}

/// `REPO_ROOT` as `lib/check.sh` derives it: `AGENTSYNC_REPO_ROOT`, else the
/// working directory, spelled logically.
fn project_root() -> Result<String, Error> {
    let root = repo_root()?;
    if !Path::new(&root).is_dir() {
        return Err(Error::ProjectRootNotFound(PathBuf::from(
            repo_root_var().unwrap_or(root),
        )));
    }
    Ok(root)
}

/// The running binary with symlinks resolved.
fn current_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
}

fn repo_root_var() -> Option<String> {
    path_setting("REPO_ROOT").filter(|root| !root.is_empty())
}

fn logical_cwd() -> Result<String, Error> {
    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
    Ok(paths::logical_root(None, &cwd, var("PWD").as_deref()))
}

/// `${AGENTSYNC_REPO_ROOT:-$PWD}`, spelled logically.
fn repo_root() -> Result<String, Error> {
    let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
    Ok(paths::logical_root(
        repo_root_var().as_deref(),
        &cwd,
        var("PWD").as_deref(),
    ))
}

/// `AGENTSYNC_REPO_ROOT` as given, else the logical working directory.
fn supplied_root() -> Result<String, Error> {
    match repo_root_var() {
        Some(root) => Ok(root),
        None => logical_cwd(),
    }
}

/// `check_for_updates`: on a terminal, unless `AGENTSYNC_NO_UPDATE_CHECK` is
/// set, the project-format notice, the banner from the cache beside the
/// install's `bin/`, and a detached `__update-cache` run that refreshes the
/// cache for the next time.
fn check_for_updates() -> Result<(), Error> {
    if !std::io::stdout().is_terminal() || setting("NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty())
    {
        return Ok(());
    }
    let style = Style::for_stdout();
    let root = repo_root()?;
    let mut out = std::io::stdout().lock();
    out.write_all(cli::notice::format_notice(Path::new(&root), &style).as_bytes())
        .map_err(|e| Error::io("<stdout>", e))?;
    let Some(cache) = current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join(cli::notice::CACHE_FILE)))
    else {
        return Ok(());
    };
    if let Ok(text) = std::fs::read_to_string(&cache) {
        out.write_all(cli::notice::update_banner(&text, engine_version(), &style).as_bytes())
            .map_err(|e| Error::io("<stdout>", e))?;
    }
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe)
            .arg("__update-cache")
            .arg(&cache)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    Ok(())
}

fn print_usage() -> Result<u8, Error> {
    let mut out = std::io::stdout().lock();
    out.write_all(cli::usage::usage(&Style::for_stdout()).as_bytes())
        .map(|()| 0)
        .map_err(|e| Error::io("<stdout>", e))
}

fn print_version() -> Result<u8, Error> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "agentsync v{}", engine_version())
        .map(|()| 0)
        .map_err(|e| Error::io("<stdout>", e))
}
