//! Compress and extract on the server (M3.3). Commands are built here with
//! every path quoted as a single shell argument, and run (as root when sudo is
//! on) as cancellable tasks recorded in Activity.

use crate::sudo::quote;
use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

/// Prefix of the error returned when the extract folder already exists.
pub const DEST_EXISTS: &str = "folder exists:";

fn check_name(n: &str) -> Result<()> {
    if n.is_empty() || n == "." || n == ".." || n.contains('/') || n.contains('\0') {
        bail!("unusable file name: {n:?}");
    }
    Ok(())
}

/// Fail with a clear message when a tool is missing on the server.
fn need(tool: &str) -> String {
    format!("command -v {tool} >/dev/null 2>&1 || {{ echo \"{tool} isn't installed on this server\" >&2; exit 127; }}")
}

/// Pack `names` (entries of `dir`) into `dir/archive`.
pub fn compress_command(
    dir: &str,
    names: &[String],
    archive: &str,
    format: ArchiveFormat,
) -> Result<String> {
    if names.is_empty() {
        bail!("nothing to compress");
    }
    check_name(archive)?;
    for n in names {
        check_name(n)?;
    }
    // "./name" keeps a name starting with "-" from being read as an option.
    let items: Vec<String> = names.iter().map(|n| quote(&format!("./{n}"))).collect();
    let target = quote(&format!("./{archive}"));
    Ok(match format {
        ArchiveFormat::TarGz => format!(
            "{} && cd {} && tar -czf {target} {}",
            need("tar"),
            quote(dir),
            items.join(" ")
        ),
        ArchiveFormat::Zip => format!(
            "{} && cd {} && zip -rq {target} {}",
            need("zip"),
            quote(dir),
            items.join(" ")
        ),
    })
}

/// The folder an archive extracts into: its name without the archive extension.
pub fn extract_folder(archive_path: &str) -> String {
    let lower = archive_path.to_lowercase();
    for ext in [
        ".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tgz", ".txz", ".tbz2", ".tar", ".zip",
    ] {
        if lower.ends_with(ext) {
            return archive_path[..archive_path.len() - ext.len()].to_string();
        }
    }
    format!("{archive_path}.d")
}

/// Unpack `archive_path` into `dest` (created first; must not exist unless `overwrite`).
pub fn extract_command(archive_path: &str, dest: &str, overwrite: bool) -> Result<String> {
    let lower = archive_path.to_lowercase();
    let (a, d) = (quote(archive_path), quote(dest));
    let unpack = if lower.ends_with(".zip") {
        format!(
            "{} && unzip -q{} {a} -d {d}",
            need("unzip"),
            if overwrite { "o" } else { "n" }
        )
    } else {
        let flag = if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            "-xzf"
        } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
            "-xJf"
        } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
            "-xjf"
        } else if lower.ends_with(".tar.zst") {
            "--zstd -xf"
        } else if lower.ends_with(".tar") {
            "-xf"
        } else {
            bail!(
                "Shellport can extract .tar, .tar.gz, .tar.xz, .tar.bz2, .tar.zst and .zip files"
            );
        };
        format!("{} && tar {flag} {a} -C {d}", need("tar"))
    };
    let mkdir = if overwrite {
        format!("mkdir -p {d}")
    } else {
        format!("mkdir {d}")
    };
    Ok(format!("{mkdir} && {unpack}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_quoted_commands() {
        let c = compress_command(
            "/srv/my app",
            &["a b".into(), "-rf".into(), "$(id)".into()],
            "out.tar.gz",
            ArchiveFormat::TarGz,
        )
        .unwrap();
        assert!(
            c.contains("cd '/srv/my app' && tar -czf './out.tar.gz' './a b' './-rf' './$(id)'"),
            "{c}"
        );
        let z = compress_command("/x", &["it's".into()], "o.zip", ArchiveFormat::Zip).unwrap();
        assert!(z.ends_with(r"zip -rq './o.zip' './it'\''s'"), "{z}");
        assert!(compress_command("/x", &["../etc".into()], "o.zip", ArchiveFormat::Zip).is_err());
        assert!(compress_command("/x", &["a".into()], "../o.zip", ArchiveFormat::Zip).is_err());
        assert!(compress_command("/x", &[], "o.zip", ArchiveFormat::Zip).is_err());
    }

    #[test]
    fn extract_targets() {
        assert_eq!(extract_folder("/srv/site.tar.gz"), "/srv/site");
        assert_eq!(extract_folder("/srv/Backup.ZIP"), "/srv/Backup");
        assert_eq!(extract_folder("/srv/odd.bin"), "/srv/odd.bin.d");
        let c = extract_command("/srv/a b.tgz", "/srv/a b", false).unwrap();
        assert!(c.starts_with("mkdir '/srv/a b' && "), "{c}");
        assert!(c.ends_with("tar -xzf '/srv/a b.tgz' -C '/srv/a b'"), "{c}");
        assert!(extract_command("/srv/x.zip", "/srv/x", true)
            .unwrap()
            .contains("unzip -qo '/srv/x.zip' -d '/srv/x'"));
        assert!(extract_command("/srv/x.7z", "/srv/x", false).is_err());
    }

    /// The commands do what they say in a real shell, with awkward names.
    #[test]
    fn round_trip_locally() {
        if std::process::Command::new("tar")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir(d.join("-odd dir")).unwrap();
        std::fs::write(d.join("-odd dir/it's.txt"), "hi").unwrap();
        let sh = |c: &str| {
            assert!(
                std::process::Command::new("sh")
                    .arg("-c")
                    .arg(c)
                    .status()
                    .unwrap()
                    .success(),
                "{c}"
            )
        };
        sh(&compress_command(
            d.to_str().unwrap(),
            &["-odd dir".into()],
            "pack.tar.gz",
            ArchiveFormat::TarGz,
        )
        .unwrap());
        let archive = d.join("pack.tar.gz").to_string_lossy().into_owned();
        let dest = extract_folder(&archive);
        sh(&extract_command(&archive, &dest, false).unwrap());
        assert_eq!(
            std::fs::read_to_string(format!("{dest}/-odd dir/it's.txt")).unwrap(),
            "hi"
        );
        // Extracting again without overwrite refuses: the folder exists.
        assert!(!std::process::Command::new("sh")
            .arg("-c")
            .arg(extract_command(&archive, &dest, false).unwrap())
            .status()
            .unwrap()
            .success());
    }
}
