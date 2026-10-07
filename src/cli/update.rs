//! `exuno update`: `lib/helpers/update.sh` for a binary install. The
//! release archive for this platform comes from GitHub Releases through
//! `curl`, its sha256 is checked in-process, `tar` unpacks it, the new binary
//! is asked for its version and its catalog, and then it is moved over the
//! running one. The changelog is the archive's; conflicts with the project's
//! overrides are queued for `resolve` as before.

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::bundle::Scratch;
use super::notice::{CACHE_FILE, REPO};
use super::put;
use crate::config::snapshot::{self, Conflict};
use crate::output::changelog;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::transaction::manifest::sha256_hex;
use crate::{Error, config::catalog, engine_version};

pub const HELP: Help = Help {
    command: "update",
    tagline: "replace the binary with a GitHub release",
    synopsis: &["update [<version>] [--strict]"],
    description: &[
        "Downloads the release archive for this platform from GitHub Releases,\nverifies its sha256, and moves the new binary over the running one.\nConflicts between the new catalog and your overrides are queued for\nexuno resolve.",
    ],
    sections: &[
        Section {
            title: "ARGUMENTS",
            entries: &[(
                "<version>",
                "Pin the install to that release tag (e.g. 0.35.0) instead of\nthe latest release — what a project's agentsync_version asks for",
            )],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--strict",
                    "Exit non-zero if upstream changed a field you have overridden",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
    ],
    examples: &["update", "update 0.35.0", "update --strict"],
};

/// The hidden command a newer binary answers with its catalog.
pub const CATALOG_COMMAND: &str = "__catalog";

/// What `update` takes from the process.
pub struct Env<'a> {
    /// The running binary with symlinks resolved: the file that is replaced.
    pub exe: PathBuf,
    /// `${AGENTSYNC_REPO_ROOT:-$(pwd)}`: the project whose overrides are checked.
    pub project_dir: String,
    /// `date -u +%Y-%m-%d`.
    pub today: String,
    /// `_changelog_width`.
    pub width: usize,
    /// Fetch `url` into the file: the HTTP status, or why curl could not answer.
    pub fetch: &'a mut dyn FnMut(&str, &Path) -> Result<u16, String>,
    /// Unpack the archive into the directory.
    pub extract: &'a mut dyn FnMut(&Path, &Path) -> bool,
    /// Run the binary with one argument: its stdout when it succeeds.
    pub ask: &'a mut dyn FnMut(&Path, &str) -> Option<String>,
}

/// The cargo-dist target this binary was built for, as its asset is named.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

const fn archive_extension() -> &'static str {
    if cfg!(windows) { "zip" } else { "tar.xz" }
}

const fn binary_name() -> &'static str {
    if cfg!(windows) { "exuno.exe" } else { "exuno" }
}

/// The shipped catalog as `(slug, yaml)` in byte order.
pub fn catalog_entries() -> Vec<(String, String)> {
    catalog::base_tools()
        .into_iter()
        .filter_map(|slug| {
            let yaml = catalog::base_tool_yaml(&slug)?.to_string();
            Some((slug, yaml))
        })
        .collect()
}

/// `__catalog`: every base tool as `<slug> <bytes>\n<yaml>\n`, so the running
/// binary can diff its catalog against a newer binary's.
pub fn catalog_dump() -> String {
    let mut out = String::new();
    for (slug, yaml) in catalog_entries() {
        out.push_str(&format!("{slug} {}\n{yaml}\n", yaml.len()));
    }
    out
}

/// The entries of a [`catalog_dump`]; `None` when the text is not one.
pub fn parse_catalog_dump(text: &str) -> Option<Vec<(String, String)>> {
    let mut entries = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (header, after) = rest.split_once('\n')?;
        let (slug, len) = header.split_once(' ')?;
        let len: usize = len.parse().ok()?;
        if slug.is_empty() || !after.is_char_boundary(len) {
            return None;
        }
        let tail = after[len..].strip_prefix('\n')?;
        entries.push((slug.to_string(), after[..len].to_string()));
        rest = tail;
    }
    Some(entries)
}

/// `curl -sL --max-time 30 -o <to> -w %{http_code} <url>`.
pub fn curl_fetch(url: &str, to: &Path) -> Result<u16, String> {
    let output = Command::new("curl")
        .args(["-sL", "--max-time", "30", "-o"])
        .arg(to)
        .args(["-w", "%{http_code}", url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let code = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .unwrap_or(0);
    if !output.status.success() || code == 0 {
        return Err(format!(
            "curl exited with status {}",
            output.status.code().unwrap_or(1)
        ));
    }
    Ok(code)
}

/// `tar -xf <archive> -C <dir>`, tar's own diagnostics passing through.
pub fn tar_extract(archive: &Path, into: &Path) -> bool {
    Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `<binary> <arg>`: its stdout when it exits 0.
pub fn ask_binary(binary: &Path, arg: &str) -> Option<String> {
    let output = Command::new(binary)
        .arg(arg)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `_changelog_width`: `tput cols` clamped, 80 without a usable answer.
pub fn terminal_width() -> usize {
    let cols = Command::new("tput")
        .arg("cols")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim_end_matches(['\n', '\r'])
                .to_string()
        });
    changelog::clamp_width(cols.as_deref())
}

/// The `.update_cache` beside the install's `bin/`, cleared once current.
fn cache_file(exe: &Path) -> Option<PathBuf> {
    Some(exe.parent()?.parent()?.join(CACHE_FILE))
}

/// `agentsync v<version>` as `version` prints it.
fn version_of(answer: &str) -> Option<String> {
    let line = answer.split('\n').next()?;
    let version = line
        .strip_prefix("exuno v")
        .or_else(|| line.strip_prefix("agentsync v"))?;
    (!version.is_empty()).then(|| version.to_string())
}

/// The unpacked binary: `exuno[.exe]` at the top or below the archive's
/// one directory, as cargo-dist lays it out.
fn unpacked_binary(dir: &Path) -> Option<PathBuf> {
    let flat = dir.join(binary_name());
    if flat.is_file() {
        return Some(flat);
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|top| top.join(binary_name()))
        .find(|path| path.is_file())
}

/// The staged copy renamed over the running binary. Windows cannot replace a
/// running executable, so there the old one is renamed aside first.
fn replace_binary(new: &Path, exe: &Path) -> Result<(), Error> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let name = exe
        .file_name()
        .map(|n| n.disk_text())
        .unwrap_or_else(|| binary_name().to_string());
    let staged = dir.join(format!(".{name}.new"));
    std::fs::copy(new, &staged).map_err(|e| Error::io(&staged, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| Error::io(&staged, e))?;
    }
    #[cfg(windows)]
    {
        let old = dir.join(format!("{name}.old"));
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| Error::io(exe, e))?;
    }
    std::fs::rename(&staged, exe).map_err(|e| Error::io(exe, e))
}

/// `_show_update_conflicts`: grouped by tool, in `LC_ALL=C sort` order.
fn conflicts_report(conflicts: &[Conflict], style: &Style) -> String {
    let mut lines: Vec<String> = conflicts
        .iter()
        .map(|c| {
            format!(
                "{}\t{}\t{}\t{}\t{}",
                c.tool, c.field, c.before, c.after, c.yours
            )
        })
        .collect();
    lines.sort_unstable();
    let unset = style.dim("(unset)");
    let shown = |value: &str| {
        if value.is_empty() {
            unset.clone()
        } else {
            value.to_string()
        }
    };
    let mut out = format!(
        "\n  {}\n\n",
        style.yellow("Upstream touched fields you have overridden:")
    );
    let mut current = String::new();
    for line in &lines {
        let mut parts = line.splitn(5, '\t');
        let tool = parts.next().unwrap_or("");
        let field = parts.next().unwrap_or("");
        let before = parts.next().unwrap_or("");
        let after = parts.next().unwrap_or("");
        let yours = parts.next().unwrap_or("");
        if tool.is_empty() {
            continue;
        }
        if tool != current {
            if !current.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("    {}\n", style.bold(tool)));
            current = tool.to_string();
        }
        out.push_str(&format!("      {} {field}\n", style.yellow("◆")));
        out.push_str(&format!(
            "          {} {} → {}\n",
            style.dim("base:"),
            shown(before),
            shown(after)
        ));
        out.push_str(&format!(
            "          {} {}\n",
            style.dim("your override:"),
            shown(yours)
        ));
    }
    out.push('\n');
    out
}

/// `_show_migration_banner`: a payload file (one with an extension) under
/// `.ai/src/{hooks,mcp,settings}/`, the files `migrate` would move.
fn migration_banner(project_dir: &Path, style: &Style) -> String {
    let src = project_dir.join(".ai/src");
    if !src.is_dir() {
        return String::new();
    }
    let found = ["hooks", "mcp", "settings"].iter().any(|resource| {
        std::fs::read_dir(src.join(resource))
            .map(|entries| {
                entries.filter_map(|entry| entry.ok()).any(|entry| {
                    let name = entry.file_name().disk_text();
                    !name.starts_with('.') && name.contains('.') && entry.path().is_file()
                })
            })
            .unwrap_or(false)
    });
    if !found {
        return String::new();
    }
    format!(
        "\n  {}\n  {} {}{}\n  {} {}{}\n  {} {} {}\n  {}\n\n",
        style.yellow("Legacy payload layout detected"),
        style.dim("Your project has overrides under"),
        style.cyan(".ai/src/{hooks,mcp,settings}/"),
        style.dim(". The canonical"),
        style.dim("layout since 0.11 is"),
        style.cyan(".ai/src/tools/<tool>/<resource>.<ext>"),
        style.dim("."),
        style.dim("Run"),
        style.cyan("exuno migrate --apply"),
        style.dim("to move them. Legacy paths still read,"),
        style.dim("but will be dropped in 0.12.")
    )
}

fn fetch_failed(err: &mut dyn Write, style: &Style, detail: &str) -> Result<u8, Error> {
    put(
        err,
        format!(
            "  {}: Failed to fetch updates from GitHub.\n    {detail}\n  {}\n",
            style.red("Error"),
            style.dim("Check your network connection and that the remote is reachable.")
        )
        .as_bytes(),
    )?;
    Ok(1)
}

fn refuse(err: &mut dyn Write, style: &Style, message: &str) -> Result<u8, Error> {
    put(
        err,
        format!("  {}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}

enum Args {
    Help,
    Refused(String),
    Run { pin: Option<String>, strict: bool },
}

fn parse_args(args: &[String]) -> Args {
    let mut strict = false;
    let mut pin = None;
    for arg in args {
        match arg.as_str() {
            "--strict" => strict = true,
            "--help" | "-h" => return Args::Help,
            flag if flag.starts_with('-') => return Args::Refused(format!("Unknown flag: {flag}")),
            word => {
                if pin.is_some() {
                    return Args::Refused(format!("Unexpected argument: {word}"));
                }
                pin = Some(word.to_string());
            }
        }
    }
    Args::Run { pin, strict }
}

/// What the download produced, once every check passed.
struct Fetched {
    new_binary: PathBuf,
    new_version: String,
    new_catalog: Vec<(String, String)>,
    changelog: Option<String>,
}

/// One release download into `scratch`, each failure reported on `err`.
struct Download<'a, 'e> {
    scratch: &'a Path,
    env: &'a mut Env<'e>,
    style: &'a Style,
    err: &'a mut dyn Write,
}

impl Download<'_, '_> {
    fn failed(&mut self, detail: &str) -> Result<u8, Error> {
        fetch_failed(self.err, self.style, detail)
    }

    fn refuse(&mut self, message: &str) -> Result<u8, Error> {
        refuse(self.err, self.style, message)
    }

    /// A fetch of `url` that did not answer 200, reported.
    fn answered(
        &mut self,
        url: &str,
        fetched: Result<u16, String>,
    ) -> Result<Result<(), u8>, Error> {
        match fetched {
            Ok(200) => Ok(Ok(())),
            Ok(code) => Ok(Err(self.failed(&format!("HTTP {code} for {url}"))?)),
            Err(why) => Ok(Err(self.failed(&why)?)),
        }
    }

    /// `url` fetched into the scratch file `name`.
    fn get(&mut self, url: &str, name: &str) -> Result<Result<PathBuf, u8>, Error> {
        let to = self.scratch.join(name);
        let fetched = (self.env.fetch)(url, &to);
        Ok(self.answered(url, fetched)?.map(|()| to))
    }

    /// The tag to install: the pin, or the latest release's.
    fn tag(&mut self, pin: Option<&str>) -> Result<Result<String, u8>, Error> {
        if let Some(pin) = pin {
            return Ok(Ok(pin.to_string()));
        }
        let url = super::notice::latest_release_url();
        let answer = match self.get(&url, "latest.json")? {
            Ok(answer) => answer,
            Err(status) => return Ok(Err(status)),
        };
        let json = std::fs::read_to_string(&answer).map_err(|e| Error::io(&answer, e))?;
        match super::notice::parse_tag_name(&json) {
            Some(tag) => Ok(Ok(tag)),
            None => Ok(Err(
                self.failed(&format!("no tag_name in the answer from {url}"))?
            )),
        }
    }

    /// The release archive; a pinned tag without one is told apart from a
    /// tag that predates the binary releases.
    fn archive(
        &mut self,
        tag: &str,
        pinned: bool,
        url: &str,
        name: &str,
    ) -> Result<Result<PathBuf, u8>, Error> {
        let archive = self.scratch.join(name);
        let fetched = (self.env.fetch)(url, &archive);
        if pinned && matches!(fetched, Ok(404)) {
            let message = self.missing_tag(tag);
            return Ok(Err(self.refuse(&message)?));
        }
        Ok(self.answered(url, fetched)?.map(|()| archive))
    }

    fn missing_tag(&mut self, tag: &str) -> String {
        let style = self.style;
        let probe = self.scratch.join("tag.json");
        let tag_url = format!("https://api.github.com/repos/{REPO}/git/ref/tags/{tag}");
        match (self.env.fetch)(&tag_url, &probe) {
            Ok(200) => format!(
                "Exuno {tag} predates the binary releases, so update cannot install it.\n  {}\n    AGENTSYNC_VERSION={tag} curl -fsSL https://raw.githubusercontent.com/{REPO}/main/install.sh | bash",
                style.dim("Pin it with the installer instead:")
            ),
            _ => format!(
                "No Exuno release is tagged {tag}.\n  {} {}",
                style.dim("List releases at"),
                style.cyan(&format!("https://github.com/{REPO}/releases"))
            ),
        }
    }

    /// The archive's sha256 against the published `.sha256` file.
    fn verify(
        &mut self,
        archive: &Path,
        name: &str,
        sum_file: &Path,
    ) -> Result<Result<(), u8>, Error> {
        let expected = std::fs::read_to_string(sum_file)
            .map_err(|e| Error::io(sum_file, e))?
            .split_ascii_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let actual = sha256_hex(&std::fs::read(archive).map_err(|e| Error::io(archive, e))?);
        if expected == actual {
            return Ok(Ok(()));
        }
        let message = format!(
            "checksum mismatch for {name}.\n  {}",
            self.style
                .dim(&format!("expected {expected}, got {actual}"))
        );
        Ok(Err(self.refuse(&message)?))
    }

    /// The archive unpacked and the binary inside it found.
    fn unpack(&mut self, archive: &Path, name: &str) -> Result<Result<PathBuf, u8>, Error> {
        let unpacked = self.scratch.join("unpacked");
        std::fs::create_dir_all(&unpacked).map_err(|e| Error::io(&unpacked, e))?;
        if !(self.env.extract)(archive, &unpacked) {
            return Ok(Err(self.refuse(&format!("could not unpack {name}."))?));
        }
        match unpacked_binary(&unpacked) {
            Some(binary) => Ok(Ok(binary)),
            None => Ok(Err(
                self.refuse(&format!("{name} does not contain {}.", binary_name()))?
            )),
        }
    }

    /// The new binary asked for its version and catalog.
    fn interrogate(&mut self, new_binary: PathBuf) -> Result<Result<Fetched, u8>, Error> {
        let Some(new_version) = (self.env.ask)(&new_binary, "version")
            .as_deref()
            .and_then(version_of)
        else {
            let message = format!(
                "the downloaded binary does not run: {}",
                new_binary.disk_text()
            );
            return Ok(Err(self.refuse(&message)?));
        };
        let Some(new_catalog) = (self.env.ask)(&new_binary, CATALOG_COMMAND)
            .as_deref()
            .and_then(parse_catalog_dump)
        else {
            let message = format!(
                "the downloaded binary did not answer {CATALOG_COMMAND}: {}",
                new_binary.disk_text()
            );
            return Ok(Err(self.refuse(&message)?));
        };
        let changelog = new_binary
            .parent()
            .and_then(|dir| std::fs::read_to_string(dir.join("CHANGELOG.md")).ok());
        Ok(Ok(Fetched {
            new_binary,
            new_version,
            new_catalog,
            changelog,
        }))
    }

    /// The archive and its checksum downloaded, verified, unpacked, and the
    /// new binary asked for its version and catalog.
    fn release(
        &mut self,
        tag: &str,
        pinned: bool,
        target: &str,
    ) -> Result<Result<Fetched, u8>, Error> {
        let archive_name = format!("exuno-{target}.{}", archive_extension());
        let base = format!("https://github.com/{REPO}/releases/download/{tag}");
        let archive_url = format!("{base}/{archive_name}");
        let archive = match self.archive(tag, pinned, &archive_url, &archive_name)? {
            Ok(archive) => archive,
            Err(status) => return Ok(Err(status)),
        };
        let sum_name = format!("{archive_name}.sha256");
        let sum_file = match self.get(&format!("{base}/{sum_name}"), &sum_name)? {
            Ok(sum_file) => sum_file,
            Err(status) => return Ok(Err(status)),
        };
        if let Err(status) = self.verify(&archive, &archive_name, &sum_file)? {
            return Ok(Err(status));
        }
        match self.unpack(&archive, &archive_name)? {
            Ok(new_binary) => self.interrogate(new_binary),
            Err(status) => Ok(Err(status)),
        }
    }
}

/// The pin and `--strict`; the help or the refusal already printed otherwise.
fn update_args(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Result<(Option<String>, bool), u8>, Error> {
    match parse_args(args) {
        Args::Help => {
            put(out, HELP.render(style).as_bytes())?;
            Ok(Err(0))
        }
        Args::Refused(message) => {
            put(
                err,
                format!(
                    "{}: {message}\nUsage: {}\n",
                    style.red("Error"),
                    HELP.synopsis_line()
                )
                .as_bytes(),
            )?;
            Ok(Err(2))
        }
        Args::Run { pin, strict } => Ok(Ok((pin, strict))),
    }
}

/// `Updated!` with the version step, then the changelog between the two.
fn updated_text(style: &Style, old_version: &str, fetched: &Fetched, width: usize) -> String {
    let new_version = fetched.new_version.as_str();
    let mut text = if old_version == new_version {
        format!("\n  {} (v{new_version})\n", style.green("Updated!"))
    } else {
        format!(
            "\n  {} v{old_version} → v{new_version}\n",
            style.green("Updated!")
        )
    };
    if let Some(changelog) = &fetched.changelog {
        let versions = changelog::versions_in_range(changelog, old_version, new_version);
        text.push_str(&changelog::sections(changelog, &versions, width, style));
    }
    text
}

fn queued_hint(style: &Style) -> String {
    format!(
        "  {} {}{} {}{}\n\n",
        style.dim("Queued in"),
        style.cyan(".ai/.pending-resolutions.yaml"),
        style.dim(" — run"),
        style.cyan("exuno resolve"),
        style.dim(" to walk them.")
    )
}

/// Shipped templates copied into `.ai/src/` change only through `refresh`.
fn refresh_hint(project_dir: &Path, style: &Style) -> String {
    if !project_dir.join(".ai/.template-manifest").is_file() {
        return String::new();
    }
    format!(
        "  {} {} {}\n\n",
        style.dim("Rule, skill, and command templates in .ai/src/ update separately — run"),
        style.cyan("exuno refresh"),
        style.dim("to review them.")
    )
}

/// `cmd_update`.
pub fn update(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (pin, strict) = match update_args(args, style, out, err)? {
        Ok(parsed) => parsed,
        Err(status) => return Ok(status),
    };
    put(
        out,
        format!(
            "\n{}\n\n  Checking for updates...\n",
            style.bold("  Exuno Update")
        )
        .as_bytes(),
    )?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let Some(target) = target() else {
        return refuse(
            err,
            style,
            &format!(
                "no release binary is built for this platform ({}/{}).",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        );
    };
    let scratch = Scratch::create("agentsync-update")?;
    let old_version = engine_version();
    let mut download = Download {
        scratch: &scratch.0,
        env,
        style,
        err,
    };
    let tag = match download.tag(pin.as_deref())? {
        Ok(tag) => tag,
        Err(status) => return Ok(status),
    };
    if tag == old_version {
        let current = format!(
            "  {} (v{old_version})\n\n",
            style.green("Already up to date!")
        );
        put(out, current.as_bytes())?;
        return Ok(0);
    }
    if pin.is_some() {
        put(out, format!("  Pinning to v{tag}...\n").as_bytes())?;
    } else {
        put(out, b"  Updating...\n")?;
    }
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let fetched = match download.release(&tag, pin.is_some(), target)? {
        Ok(fetched) => fetched,
        Err(status) => return Ok(status),
    };
    let project_dir = Path::new(&env.project_dir);
    let changes = snapshot::diff(&catalog_entries(), &fetched.new_catalog);
    let conflicts = snapshot::find_conflicts(project_dir, &changes);
    replace_binary(&fetched.new_binary, &env.exe)?;
    if let Some(cache) = cache_file(&env.exe) {
        let _ = std::fs::remove_file(cache);
    }
    put(
        out,
        updated_text(style, old_version, &fetched, env.width).as_bytes(),
    )?;
    if !conflicts.is_empty() {
        put(out, conflicts_report(&conflicts, style).as_bytes())?;
        if project_dir.join(".ai").is_dir() {
            snapshot::write_pending_resolutions(
                project_dir,
                &env.today,
                old_version,
                &fetched.new_version,
                &conflicts,
            )?;
            put(out, queued_hint(style).as_bytes())?;
        }
    }
    put(out, b"\n")?;
    put(out, refresh_hint(project_dir, style).as_bytes())?;
    put(out, migration_banner(project_dir, style).as_bytes())?;
    Ok(u8::from(strict && !conflicts.is_empty()))
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn the_catalog_dump_round_trips_the_fifteen_tools() {
        let entries = catalog_entries();
        assert_eq!(entries.len(), 15);
        let dump = catalog_dump();
        assert!(dump.starts_with("amazonq "));
        assert_eq!(parse_catalog_dump(&dump), Some(entries));
        assert_eq!(parse_catalog_dump(""), Some(Vec::new()));
        assert_eq!(parse_catalog_dump("claude 3\nab\n"), None);
        assert_eq!(parse_catalog_dump("claude x\n"), None);
        assert_eq!(
            parse_catalog_dump("a 2\nxy\nb 0\n\n"),
            Some(vec![
                ("a".to_string(), "xy".to_string()),
                ("b".to_string(), String::new())
            ])
        );
    }

    #[test]
    fn the_target_is_one_of_the_five_dist_targets() {
        let target = target().expect("a supported host");
        assert!(
            [
                "aarch64-apple-darwin",
                "x86_64-apple-darwin",
                "aarch64-unknown-linux-musl",
                "x86_64-unknown-linux-musl",
                "x86_64-pc-windows-msvc",
            ]
            .contains(&target)
        );
        assert_eq!(
            version_of("agentsync v0.36.0\n"),
            Some("0.36.0".to_string())
        );
        assert_eq!(version_of("agentsync v"), None);
        assert_eq!(version_of("exuno v1.2.3\n").as_deref(), Some("1.2.3"));
        assert_eq!(version_of("exuno v"), None);
        assert_eq!(version_of("nope"), None);
    }

    #[test]
    fn conflicts_are_grouped_by_tool_in_byte_order_with_unset_marked() {
        let conflicts = [
            Conflict {
                tool: "cursor".to_string(),
                field: "targets.rules.dest".to_string(),
                before: ".cursor/rules".to_string(),
                after: ".cursor/rules-v2".to_string(),
                yours: "mine".to_string(),
            },
            Conflict {
                tool: "claude".to_string(),
                field: "name".to_string(),
                before: String::new(),
                after: "Claude".to_string(),
                yours: "Mine".to_string(),
            },
        ];
        assert_eq!(
            conflicts_report(&conflicts, &Style::plain()),
            "\n  Upstream touched fields you have overridden:\n\n    claude\n      ◆ name\n          base: (unset) → Claude\n          your override: Mine\n\n    cursor\n      ◆ targets.rules.dest\n          base: .cursor/rules → .cursor/rules-v2\n          your override: mine\n\n"
        );
    }

    #[test]
    fn the_migration_banner_needs_a_visible_file_under_a_legacy_directory() {
        let dir = tempfile::tempdir().unwrap();
        let style = Style::plain();
        assert_eq!(migration_banner(dir.path(), &style), "");
        std::fs::create_dir_all(dir.path().join(".ai/src/mcp")).unwrap();
        std::fs::write(dir.path().join(".ai/src/mcp/.keep"), "").unwrap();
        std::fs::write(dir.path().join(".ai/src/mcp/README"), "").unwrap();
        assert_eq!(migration_banner(dir.path(), &style), "");
        std::fs::write(dir.path().join(".ai/src/mcp/claude.json"), "{}").unwrap();
        assert_eq!(
            migration_banner(dir.path(), &style),
            "\n  Legacy payload layout detected\n  Your project has overrides under .ai/src/{hooks,mcp,settings}/. The canonical\n  layout since 0.11 is .ai/src/tools/<tool>/<resource>.<ext>.\n  Run exuno migrate --apply to move them. Legacy paths still read,\n  but will be dropped in 0.12.\n\n"
        );
    }

    /// A fake GitHub: URL to file bytes, served through the `fetch` seam; a
    /// fake archive whose "extraction" copies the staged directory; a fake
    /// binary answering `version` and `__catalog` from what the test staged.
    struct Fixture {
        dir: tempfile::TempDir,
        served: BTreeMap<String, Vec<u8>>,
        release_dir: PathBuf,
        version: String,
        catalog: String,
        offline: bool,
    }

    impl Fixture {
        fn new(version: &str, catalog: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("install/bin")).unwrap();
            std::fs::write(dir.path().join("install/bin/agentsync"), "old binary").unwrap();
            std::fs::write(dir.path().join("install/.update_cache"), "9.9.9\n").unwrap();
            std::fs::create_dir_all(dir.path().join("project/.ai/src/tools")).unwrap();
            let release_dir = dir.path().join("release/exuno-fixture");
            std::fs::create_dir_all(&release_dir).unwrap();
            std::fs::write(release_dir.join("exuno"), "new binary").unwrap();
            std::fs::write(
                release_dir.join("CHANGELOG.md"),
                format!("# Changelog\n\n## {version}\n\n### Fixed\n\n- **Something** with `code`.\n\n## 0.1.0\n\n- Ancient.\n"),
            )
            .unwrap();
            Self {
                dir,
                served: BTreeMap::new(),
                release_dir,
                version: version.to_string(),
                catalog: catalog.to_string(),
                offline: false,
            }
        }

        fn exe(&self) -> PathBuf {
            self.dir.path().join("install/bin/agentsync")
        }

        fn project(&self) -> PathBuf {
            self.dir.path().join("project")
        }

        fn publish(&mut self, tag: &str) {
            let target = target().unwrap();
            let base = format!("https://github.com/{REPO}/releases/download/{tag}");
            let archive = b"an archive".to_vec();
            let sum = format!("{}  exuno-{target}.tar.xz\n", sha256_hex(&archive));
            self.served
                .insert(format!("{base}/exuno-{target}.tar.xz"), archive);
            self.served.insert(
                format!("{base}/exuno-{target}.tar.xz.sha256"),
                sum.into_bytes(),
            );
        }

        fn run(&mut self, args: &[&str]) -> (u8, String, String) {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let served = self.served.clone();
            let offline = self.offline;
            let mut fetch = |url: &str, to: &Path| -> Result<u16, String> {
                if offline {
                    return Err("curl exited with status 6".to_string());
                }
                match served.get(url) {
                    Some(bytes) => {
                        std::fs::write(to, bytes).unwrap();
                        Ok(200)
                    }
                    None => Ok(404),
                }
            };
            let release_dir = self.release_dir.clone();
            let mut extract = |_archive: &Path, into: &Path| -> bool {
                let top = into.join("exuno-fixture");
                std::fs::create_dir_all(&top).unwrap();
                for name in ["exuno", "CHANGELOG.md"] {
                    std::fs::copy(release_dir.join(name), top.join(name)).unwrap();
                }
                true
            };
            let (version, catalog) = (self.version.clone(), self.catalog.clone());
            let mut ask = |_binary: &Path, arg: &str| -> Option<String> {
                match arg {
                    "version" => Some(format!("exuno v{version}\n")),
                    CATALOG_COMMAND => Some(catalog.clone()),
                    _ => None,
                }
            };
            let mut env = Env {
                exe: self.exe(),
                project_dir: self.project().disk_text(),
                today: "2026-09-18".to_string(),
                width: 80,
                fetch: &mut fetch,
                extract: &mut extract,
                ask: &mut ask,
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let status = update(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
            (
                status,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        }
    }

    #[test]
    fn help_and_bad_arguments_answer_like_cmd_update() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, out, err) = fixture.run(&["--help"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\n  exuno update — replace the binary with a GitHub release\n\n  USAGE\n    exuno update [<version>] [--strict]\n\n  DESCRIPTION\n    Downloads the release archive for this platform from GitHub Releases,\n    verifies its sha256, and moves the new binary over the running one.\n    Conflicts between the new catalog and your overrides are queued for\n    exuno resolve.\n\n  ARGUMENTS\n    <version>   Pin the install to that release tag (e.g. 0.35.0) instead of\n                the latest release — what a project's agentsync_version asks for\n\n  OPTIONS\n    --strict     Exit non-zero if upstream changed a field you have overridden\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno update\n    exuno update 0.35.0\n    exuno update --strict\n\n"
        );
        let (status, out, err) = fixture.run(&["--bogus"]);
        assert_eq!(
            (status, out.as_str(), err.as_str()),
            (
                2,
                "",
                "Error: Unknown flag: --bogus\nUsage: exuno update [<version>] [--strict]\n"
            )
        );
        let (status, _, err) = fixture.run(&["1.0.0", "2.0.0"]);
        assert_eq!(
            (status, err.as_str()),
            (
                2,
                "Error: Unexpected argument: 2.0.0\nUsage: exuno update [<version>] [--strict]\n"
            )
        );
    }

    #[test]
    fn the_refresh_hint_needs_a_template_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let style = Style::plain();
        assert_eq!(refresh_hint(dir.path(), &style), "");
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(dir.path().join(".ai/.template-manifest"), "").unwrap();
        assert_eq!(
            refresh_hint(dir.path(), &style),
            "  Rule, skill, and command templates in .ai/src/ update separately — run exuno refresh to review them.\n\n"
        );
    }

    #[test]
    fn the_latest_release_replaces_the_binary_and_prints_its_changelog() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        fixture.served.insert(
            super::super::notice::latest_release_url(),
            b"{\"tag_name\":\"9.9.9\"}".to_vec(),
        );
        fixture.publish("9.9.9");
        let (status, out, err) = fixture.run(&[]);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  Exuno Update\n\n  Checking for updates...\n  Updating...\n\n  Updated! v{0} → v9.9.9\n\n  What's new in v9.9.9:\n\n\n  Fixed\n    • Something with code.\n\n",
                engine_version()
            )
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "new binary"
        );
        assert!(!fixture.dir.path().join("install/.update_cache").exists());
        assert!(
            !fixture
                .project()
                .join(".ai/.pending-resolutions.yaml")
                .exists()
        );
    }

    #[test]
    fn the_running_version_is_already_up_to_date() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, out, err) = fixture.run(&[engine_version()]);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  Exuno Update\n\n  Checking for updates...\n  Already up to date! (v{})\n\n",
                engine_version()
            )
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_pin_reports_an_unknown_tag_a_pre_binary_tag_and_a_bad_checksum() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, out, err) = fixture.run(&["999.0.0"]);
        assert_eq!(status, 1);
        assert!(out.ends_with("  Pinning to v999.0.0...\n"));
        assert_eq!(
            err,
            "  Error: No Exuno release is tagged 999.0.0.\n  List releases at https://github.com/yelmuratoff/exuno/releases\n"
        );
        fixture.served.insert(
            format!("https://api.github.com/repos/{REPO}/git/ref/tags/0.1.0"),
            b"{\"ref\":\"refs/tags/0.1.0\"}".to_vec(),
        );
        let (status, _, err) = fixture.run(&["0.1.0"]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "  Error: Exuno 0.1.0 predates the binary releases, so update cannot install it.\n  Pin it with the installer instead:\n    AGENTSYNC_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash\n"
        );
        fixture.publish("9.9.9");
        let target = target().unwrap();
        fixture.served.insert(
            format!(
                "https://github.com/{REPO}/releases/download/9.9.9/exuno-{target}.tar.xz.sha256"
            ),
            b"0000  nope\n".to_vec(),
        );
        let (status, _, err) = fixture.run(&["9.9.9"]);
        assert_eq!(status, 1);
        assert!(err.starts_with(&format!(
            "  Error: checksum mismatch for exuno-{target}.tar.xz.\n  expected 0000, got "
        )));
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_downloaded_binary_that_does_not_answer_is_refused() {
        let mut fixture = Fixture::new("", &catalog_dump());
        fixture.publish("9.9.9");
        let (status, _, err) = fixture.run(&["9.9.9"]);
        assert_eq!(status, 1);
        assert!(
            err.starts_with("  Error: the downloaded binary does not run: "),
            "{err}"
        );
        let mut fixture = Fixture::new("9.9.9", "not a catalog");
        fixture.publish("9.9.9");
        let (status, _, err) = fixture.run(&["9.9.9"]);
        assert_eq!(status, 1);
        assert!(
            err.starts_with("  Error: the downloaded binary did not answer __catalog: "),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_fetch_failure_names_the_cause() {
        let mut fixture = Fixture::new("9.9.9", &catalog_dump());
        let (status, _, err) = fixture.run(&[]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "  Error: Failed to fetch updates from GitHub.\n    HTTP 404 for https://api.github.com/repos/yelmuratoff/exuno/releases/latest\n  Check your network connection and that the remote is reachable.\n"
        );
        fixture.offline = true;
        let (status, _, err) = fixture.run(&["9.9.9"]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            "  Error: Failed to fetch updates from GitHub.\n    curl exited with status 6\n  Check your network connection and that the remote is reachable.\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "old binary"
        );
    }

    #[test]
    fn a_changed_overridden_field_is_reported_queued_and_fails_strict() {
        let mut catalog = catalog_dump();
        let claude = catalog::base_tool_yaml("claude").unwrap();
        let changed = claude.replacen(".claude/rules", ".claude/rules-v2", 1);
        assert_ne!(claude, changed);
        catalog = catalog.replace(
            &format!("claude {}\n{claude}", claude.len()),
            &format!("claude {}\n{changed}", changed.len()),
        );
        let mut fixture = Fixture::new("9.9.9", &catalog);
        fixture.publish("9.9.9");
        std::fs::write(
            fixture.project().join(".ai/src/tools/claude.yaml"),
            "targets:\n  rules:\n    dest: \".claude/my-rules\"\n",
        )
        .unwrap();
        let (status, out, err) = fixture.run(&["9.9.9"]);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert!(out.contains("\n  Upstream touched fields you have overridden:\n\n    claude\n      ◆ targets.rules.dest\n          base: .claude/rules → .claude/rules-v2\n          your override: .claude/my-rules\n\n  Queued in .ai/.pending-resolutions.yaml — run exuno resolve to walk them.\n\n\n"));
        let queue =
            std::fs::read_to_string(fixture.project().join(".ai/.pending-resolutions.yaml"))
                .unwrap();
        assert!(queue.contains(&format!(
            "from_version: \"{}\"\nto_version: \"9.9.9\"\n",
            engine_version()
        )));
        assert!(queue.contains("    your_override: \".claude/my-rules\"\n"));
        std::fs::write(fixture.exe(), "old binary").unwrap();
        let (status, _, _) = fixture.run(&["9.9.9", "--strict"]);
        assert_eq!(status, 1);
        assert_eq!(
            std::fs::read_to_string(fixture.exe()).unwrap(),
            "new binary"
        );
    }
}
