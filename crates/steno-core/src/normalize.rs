use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

static RE_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static RE_HEX_LONG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{40}\b").unwrap());
static RE_HEX_SHORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,12}\b").unwrap());
static RE_UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
});
static RE_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?\b").unwrap()
});
static RE_NUMERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{4,}\b").unwrap());
static RE_QUOTED_DOUBLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""[^"]*""#).unwrap());
static RE_QUOTED_SINGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"'[^']*'").unwrap());
static RE_ABS_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(/[^\s/]+)+/([^\s/]+)").unwrap());

// A bare filename qualifies as path-like only if it carries an extension that
// contains at least one letter (so `README.md`, `foo.txt`, `x.7z` match but
// version-like `v1.2` / `3.14` do not).
static RE_BARE_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.[0-9]*[A-Za-z][A-Za-z0-9]*$").unwrap());
// `VAR=value` env-assignment prefix.
static RE_ENV_ASSIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*=").unwrap());
// `--option=value` long-option prefix.
static RE_LONG_OPT_ASSIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^--[^=]+=").unwrap());

pub fn normalize_command(cmd: &str) -> String {
    let mut result = cmd.to_string();

    // 1. Collapse whitespace
    result = RE_WHITESPACE.replace_all(&result, " ").to_string();
    result = result.trim().to_string();

    // 2. Replace UUIDs (before hex hashes, since UUIDs contain hex)
    result = RE_UUID.replace_all(&result, "<UUID>").to_string();

    // 3. Replace ISO timestamps
    result = RE_TIMESTAMP.replace_all(&result, "<TIMESTAMP>").to_string();

    // 4. Replace hex hashes (40-char first, then 7-12 char)
    result = RE_HEX_LONG.replace_all(&result, "<HASH>").to_string();
    result = RE_HEX_SHORT.replace_all(&result, "<HASH>").to_string();

    // 5. Replace quoted strings
    result = RE_QUOTED_DOUBLE.replace_all(&result, "<STR>").to_string();
    result = RE_QUOTED_SINGLE.replace_all(&result, "<STR>").to_string();

    // 6. Replace absolute paths (preserve basename)
    result = RE_ABS_PATH
        .replace_all(&result, |caps: &regex::Captures| {
            format!("<PATH>/{}", &caps[2])
        })
        .to_string();

    // 7. Replace long numeric tokens
    result = RE_NUMERIC.replace_all(&result, "<NUM>").to_string();

    // 8. Sort flags alphabetically within the command
    sort_flags(&result)
}

fn sort_flags(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return cmd.to_string();
    }

    let mut command_parts = Vec::new();
    let mut flags: Vec<&str> = Vec::new();
    let mut args: Vec<&str> = Vec::new();
    let mut past_command = false;

    for part in &parts {
        if !past_command && !part.starts_with('-') {
            command_parts.push(*part);
            if command_parts.len() >= 2 || !is_known_prefix(command_parts[0]) {
                past_command = true;
            }
        } else if part.starts_with('-') {
            past_command = true;
            flags.push(part);
        } else {
            past_command = true;
            args.push(part);
        }
    }

    flags.sort();

    let mut result = command_parts;
    result.extend(flags);
    result.extend(args);
    result.join(" ")
}

fn is_known_prefix(cmd: &str) -> bool {
    matches!(
        cmd,
        "git"
            | "cargo"
            | "docker"
            | "kubectl"
            | "npm"
            | "yarn"
            | "pip"
            | "apt"
            | "dnf"
            | "pacman"
            | "systemctl"
            | "journalctl"
            | "make"
            | "sudo"
    )
}

/// Rewrite relative path tokens in `cmd` to absolute paths rooted at `cwd`.
///
/// Only tokens that are path-like *and* resolve to an existing file, directory,
/// or symlink are rewritten; everything else — including whitespace runs — is
/// preserved byte-for-byte. This is a whitespace-token heuristic gated on
/// filesystem existence, **not** a shell parser: quoted paths containing spaces,
/// punctuation-adjacent paths, redirects, and glob/substitution constructs are
/// intentionally left untouched (their tokens fail the existence gate).
pub fn absolutize_command_paths(cmd: &str, cwd: &str) -> String {
    let mut result = String::with_capacity(cmd.len());
    let mut token_start: Option<usize> = None;

    for (i, ch) in cmd.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                result.push_str(&rewrite_token(&cmd[start..i], cwd));
            }
            result.push(ch);
        } else if token_start.is_none() {
            token_start = Some(i);
        }
    }
    if let Some(start) = token_start.take() {
        result.push_str(&rewrite_token(&cmd[start..], cwd));
    }

    result
}

/// Rewrite a single whitespace-delimited token, splitting off an env-var
/// (`VAR=path`) or long-option (`--out=path`) prefix so only the value is
/// treated as a path candidate. Returns the token unchanged when it does not
/// resolve to an existing relative path.
fn rewrite_token(token: &str, cwd: &str) -> String {
    let split = RE_ENV_ASSIGN
        .find(token)
        .or_else(|| RE_LONG_OPT_ASSIGN.find(token))
        .map(|m| m.end());
    let (prefix, candidate) = match split {
        Some(end) => (&token[..end], &token[end..]),
        None => ("", token),
    };

    match absolutize_candidate(candidate, cwd) {
        Some(abs) => format!("{prefix}{abs}"),
        None => token.to_string(),
    }
}

/// Resolve a path candidate to a cwd-rooted absolute path when it is path-like
/// and exists on disk; otherwise `None`.
fn absolutize_candidate(candidate: &str, cwd: &str) -> Option<String> {
    if candidate.is_empty()
        || candidate.starts_with('/') // already absolute
        || candidate.starts_with('-') // flag
        || candidate.contains("://")
    // URL
    {
        return None;
    }

    // Path-like: contains a slash, or is a bare filename with a lettered extension.
    let path_like = candidate.contains('/') || RE_BARE_FILE.is_match(candidate);
    if !path_like {
        return None;
    }

    let joined = Path::new(cwd).join(candidate);

    // Existence gate via lstat — matches files, directories, and broken symlinks.
    if joined.symlink_metadata().is_err() {
        return None;
    }

    Some(lexical_normalize(&joined).to_string_lossy().into_owned())
}

/// Collapse `.` and `..` components lexically, without resolving symlinks.
/// The base is always an absolute cwd, so `pop()` is a no-op at root and `..`
/// can never escape above `/` (e.g. `/tmp/../../x` → `/x`).
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_whitespace() {
        assert_eq!(
            normalize_command("git  commit   -m  <STR>"),
            "git commit -m <STR>"
        );
    }

    #[test]
    fn test_replace_uuid() {
        let cmd = "docker run 550e8400-e29b-41d4-a716-446655440000";
        let result = normalize_command(cmd);
        assert!(result.contains("<UUID>"));
        assert!(!result.contains("550e8400"));
    }

    #[test]
    fn test_replace_hex_hash() {
        let cmd = "git show a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let result = normalize_command(cmd);
        assert!(result.contains("<HASH>"));
    }

    #[test]
    fn test_replace_quoted_strings() {
        let cmd = r#"git commit -m "my commit message""#;
        let result = normalize_command(cmd);
        assert!(result.contains("<STR>"));
    }

    #[test]
    fn test_replace_absolute_path() {
        let cmd = "cat /home/user/project/file.txt";
        let result = normalize_command(cmd);
        assert!(result.contains("<PATH>/file.txt"));
    }

    #[test]
    fn test_sort_flags() {
        let cmd = "git commit -v -a -m <STR>";
        let result = normalize_command(cmd);
        assert_eq!(result, "git commit -a -m -v <STR>");
    }

    // ---- absolutize_command_paths -------------------------------------------

    use std::path::{Path, PathBuf};

    /// Create a fresh, uniquely-named temp directory to use as a `cwd`.
    fn temp_cwd(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("steno_abs_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn test_multiple_path_args_all_rewritten() {
        // The central regression: prior code only fixed the first token.
        let cwd = temp_cwd("multi");
        touch(&cwd.join("src/a.txt"));
        touch(&cwd.join("dst/b.txt"));

        let out = absolutize_command_paths("cp src/a.txt dst/b.txt", cwd.to_str().unwrap());
        assert_eq!(
            out,
            format!("cp {0}/src/a.txt {0}/dst/b.txt", cwd.display())
        );
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn test_slash_dotslash_and_parent_collapse() {
        let base = temp_cwd("collapse");
        let sub = base.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        touch(&sub.join("zarf/x/setup.sh"));
        touch(&base.join("sib/y.txt"));

        let cwd = sub.to_str().unwrap();
        // slash path + ./ prefix collapse
        assert_eq!(
            absolutize_command_paths("./zarf/x/setup.sh", cwd),
            format!("{}/zarf/x/setup.sh", sub.display())
        );
        // ../ collapse resolves into the sibling under base
        assert_eq!(
            absolutize_command_paths("cat ../sib/y.txt", cwd),
            format!("cat {}/sib/y.txt", base.display())
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_bare_filename_with_extension() {
        let cwd = temp_cwd("bare");
        touch(&cwd.join("README.md"));
        let c = cwd.to_str().unwrap();

        assert_eq!(
            absolutize_command_paths("grep TODO README.md", c),
            format!("grep TODO {}/README.md", cwd.display())
        );
        // Version-like tokens are never path-like (extension has no letter),
        // so they are left alone even if such a file existed.
        assert_eq!(absolutize_command_paths("echo v1.2", c), "echo v1.2");
        assert_eq!(absolutize_command_paths("echo 3.14", c), "echo 3.14");
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn test_option_value_keeps_prefix() {
        let cwd = temp_cwd("optval");
        touch(&cwd.join("build/out.bin"));
        touch(&cwd.join("conf/app.toml"));
        let c = cwd.to_str().unwrap();

        // attached long-option form
        assert_eq!(
            absolutize_command_paths("tool --out=build/out.bin", c),
            format!("tool --out={}/build/out.bin", cwd.display())
        );
        // env-assignment form
        assert_eq!(
            absolutize_command_paths("CONF=conf/app.toml run", c),
            format!("CONF={}/conf/app.toml run", cwd.display())
        );
        // separate-arg form works because the path is its own token
        assert_eq!(
            absolutize_command_paths("tool --out build/out.bin", c),
            format!("tool --out {}/build/out.bin", cwd.display())
        );
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn test_broken_symlink_is_absolutized() {
        let cwd = temp_cwd("symlink");
        std::os::unix::fs::symlink("/nonexistent/target", cwd.join("dangling")).unwrap();
        let c = cwd.to_str().unwrap();

        assert_eq!(
            absolutize_command_paths("rm ./dangling", c),
            format!("rm {}/dangling", cwd.display())
        );
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn test_left_unchanged() {
        let cwd = temp_cwd("unchanged");
        // A real file exists, but the tokens below should still be left alone.
        touch(&cwd.join("src/foo.rs"));
        let c = cwd.to_str().unwrap();

        for cmd in [
            "cat foo/bar",        // does not exist
            "cargo test",         // bare, no extension
            "make build",         // bare, no extension
            "cat /etc/passwd",    // already absolute
            "cat ~/notes.txt",    // home-relative, unresolved
            "curl https://a/b/c", // URL
            "cat src/foo.rs;",    // punctuation-adjacent → existence fails
        ] {
            assert_eq!(absolutize_command_paths(cmd, c), cmd, "cmd: {cmd}");
        }
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn test_whitespace_preserved_byte_for_byte() {
        let cwd = temp_cwd("ws");
        touch(&cwd.join("a.txt"));
        let c = cwd.to_str().unwrap();

        // Two spaces and a tab between tokens must survive verbatim.
        let out = absolutize_command_paths("cat  a.txt\tb", c);
        assert_eq!(out, format!("cat  {}/a.txt\tb", cwd.display()));
        std::fs::remove_dir_all(&cwd).ok();
    }

    #[test]
    fn test_lexical_normalize_cannot_escape_root() {
        assert_eq!(
            lexical_normalize(Path::new("/tmp/../../x")),
            PathBuf::from("/x")
        );
        assert_eq!(
            lexical_normalize(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
    }
}
