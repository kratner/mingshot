//! Where a capture goes after it's made (M1, PLAN.md §2 "Output"): the clipboard, a PNG in the MingShot folder
//! (~/Pictures/MingShot, Keith 2026-11-26), or both; and afterwards Show in Finder and Open.

use std::path::{Path, PathBuf};

/// What happens to a capture: the capture window's "After capture" setting.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum After {
    Copy,
    Save,
    Both,
}

impl After {
    pub fn copies(self) -> bool {
        matches!(self, After::Copy | After::Both)
    }
    pub fn saves(self) -> bool {
        matches!(self, After::Save | After::Both)
    }
}

/// ~/Pictures/MingShot (made when first needed).
pub fn folder() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("no home folder")?;
    let dir = PathBuf::from(home).join("Pictures").join("MingShot");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    Ok(dir)
}

/// "MingShot 2026-11-26 at 17.42.05", like the Mac's own "Screenshot … at …" names, in local time.
pub fn base_name() -> String {
    use objc2_foundation::{NSDate, NSDateFormatter, NSLocale, NSString};
    let f = NSDateFormatter::new();
    // A fixed locale so the digits and separators never depend on the user's region settings.
    f.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(
        &NSString::from_str("en_US_POSIX"),
    )));
    f.setDateFormat(Some(&NSString::from_str("yyyy-MM-dd 'at' HH.mm.ss")));
    format!("MingShot {}", f.stringFromDate(&NSDate::date()))
}

/// `dir/base.png`, or `dir/base (2).png`, `(3)`… if that's taken.
pub fn free_path(dir: &Path, base: &str) -> PathBuf {
    let first = dir.join(format!("{base}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{base} ({n}).png")))
        .find(|p| !p.exists())
        .expect("some number is free")
}

/// Writes the PNG into the MingShot folder; returns where.
pub fn save(png: &[u8]) -> Result<PathBuf, String> {
    let path = free_path(&folder()?, &base_name());
    std::fs::write(&path, png).map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
    Ok(path)
}

/// Shows the file selected in a Finder window.
pub fn reveal(path: &Path) -> Result<(), String> {
    run("open", &["-R".as_ref(), path.as_os_str()])
}

/// Opens the file in the Mac's default app for PNGs (Preview, until MingShot's editor arrives in M2).
pub fn open(path: &Path) -> Result<(), String> {
    run("open", &[path.as_os_str()])
}

fn run(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let ok = std::process::Command::new(cmd)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(format!("{cmd} failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_look_like_the_macs() {
        let n = base_name();
        assert!(n.starts_with("MingShot "), "{n}");
        // "MingShot 2026-11-26 at 17.42.05"
        let rest = &n["MingShot ".len()..];
        assert_eq!(rest.len(), "2026-11-26 at 17.42.05".len(), "{n}");
        assert_eq!(&rest[10..14], " at ", "{n}");
        assert!(!n.contains('/') && !n.contains(':'), "{n}");
    }

    #[test]
    fn taken_names_get_a_number() {
        let dir = std::env::temp_dir().join(format!("mingshot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = free_path(&dir, "MingShot x");
        assert_eq!(a.file_name().unwrap(), "MingShot x.png");
        std::fs::write(&a, b"1").unwrap();
        let b = free_path(&dir, "MingShot x");
        assert_eq!(b.file_name().unwrap(), "MingShot x (2).png");
        std::fs::write(&b, b"2").unwrap();
        assert_eq!(
            free_path(&dir, "MingShot x").file_name().unwrap(),
            "MingShot x (3).png"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn after_choices() {
        assert!(After::Copy.copies() && !After::Copy.saves());
        assert!(!After::Save.copies() && After::Save.saves());
        assert!(After::Both.copies() && After::Both.saves());
    }
}
