// Корзина по спецификации freedesktop: ~/.local/share/Trash для домашнего диска,
// <точка монтирования>/.Trash-<uid> для остальных. Общая с Dolphin, Nautilus, gio trash.
// Положить в неё и проверки корзины на чужом диске — yami_px::trash (то же у yami-view); здесь —
// список, возврат, очистка.

use crate::fsx::Entry;
use std::path::{Path, PathBuf};
pub use yami_px::trash::home_trash;
use yami_px::trash::{real_trash, uid};

/// откуда удалён файл по строке «Path=» его .trashinfo. Домашняя корзина (top нет) — путь как записан.
/// Корзина на чужом диске — только место на этом же диске: путь относительно его корня и не выше его
/// («../../.bashrc»), а абсолютный — лишь когда лежит на нём; иначе чужой .trashinfo «вернул» бы файл
/// в домашнюю папку (в автозапуск, например)
fn orig_of(top: Option<&Path>, v: &str) -> Option<PathBuf> {
    let p = dec(v);
    let Some(top) = top else { return Some(p) };
    let rel = if v.starts_with('/') { p.strip_prefix(top).ok()?.to_path_buf() } else { p.strip_prefix("/").unwrap_or(&p).to_path_buf() };
    (rel.components().next().is_some() && rel.components().all(|c| matches!(c, std::path::Component::Normal(_)))).then(|| top.join(rel))
}

fn dec(s: &str) -> PathBuf {
    crate::icons::uri_path(&format!("file://{}", if s.starts_with('/') { s.to_string() } else { format!("/{s}") })).unwrap_or_else(|| PathBuf::from(s))
}

fn parse_stamp(s: &str) -> i64 {
    let n: Vec<i32> = s.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
    if n.len() < 6 {
        return 0;
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = n[0] - 1900;
    tm.tm_mon = n[1] - 1;
    tm.tm_mday = n[2];
    tm.tm_hour = n[3];
    tm.tm_min = n[4];
    tm.tm_sec = n[5];
    tm.tm_isdst = -1;
    unsafe { libc::mktime(&mut tm) as i64 }
}

/// в корзину; Ok(путь в корзине)
pub fn put(p: &Path) -> Result<PathBuf, String> {
    yami_px::trash::put(p).map_err(|e| match e {
        yami_px::trash::PutErr::NoTrash => crate::i18n::tr("корзина недоступна").to_string(),
        yami_px::trash::PutErr::Io(e) => crate::fsx::err_text(&e),
    })
}

/// файловая система диска (своя корзина .Trash-UID бывает только у таких)
fn local_fs(fs: &str) -> bool {
    matches!(fs, "ext2" | "ext3" | "ext4" | "btrfs" | "xfs" | "f2fs" | "vfat" | "exfat" | "ntfs" | "ntfs3" | "fuseblk" | "hfsplus" | "jfs" | "reiserfs" | "zfs" | "bcachefs" | "nilfs2" | "udf" | "msdos")
}

fn trash_dirs() -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut v = vec![(home_trash(), None)];
    if let Ok(m) = std::fs::read_to_string("/proc/self/mounts") {
        for l in m.lines() {
            let mut f = l.split(' ');
            let (Some(_), Some(mp), Some(fs)) = (f.next(), f.next(), f.next()) else { continue };
            // только диски: служебные ФС смотреть незачем, а stat на зависшей сети (nfs, smb,
            // sshfs, gvfs) повесил бы окно — подсчёт корзины идёт при открытии и в кадре
            if !local_fs(fs) {
                continue;
            }
            let mp = PathBuf::from(mp.replace("\\040", " "));
            let t = mp.join(format!(".Trash-{}", uid()));
            // только настоящая: корзину из ссылок (real_trash) не показываем и не чистим
            if real_trash(&t) && !v.iter().any(|(x, _)| *x == t) {
                v.push((t, Some(mp)));
            }
        }
    }
    v
}

pub fn list() -> Vec<Entry> {
    let mut out = vec![];
    for (t, top) in trash_dirs() {
        let Ok(rd) = std::fs::read_dir(t.join("files")) else { continue };
        for e in rd.flatten() {
            let Some(mut en) = Entry::from_path(&e.path()) else { continue };
            let info = t.join("info").join(format!("{}.trashinfo", en.name));
            if let Ok(txt) = std::fs::read_to_string(&info) {
                for l in txt.lines() {
                    if let Some(v) = l.strip_prefix("Path=") {
                        en.orig = orig_of(top.as_deref(), v);
                    } else if let Some(v) = l.strip_prefix("DeletionDate=") {
                        en.deleted = parse_stamp(v);
                    }
                }
            }
            out.push(en);
        }
    }
    out
}

pub fn count() -> usize {
    trash_dirs().iter().map(|(t, _)| std::fs::read_dir(t.join("files")).map(|r| r.count()).unwrap_or(0)).sum()
}

fn info_of(p: &Path) -> PathBuf {
    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    p.parent().and_then(|f| f.parent()).map(|t| t.join("info").join(format!("{name}.trashinfo"))).unwrap_or_default()
}

/// вернуть на место; занято — рядом под свободным именем
pub fn restore(e: &Entry) -> Result<PathBuf, String> {
    let orig = e.orig.clone().ok_or_else(|| crate::i18n::tr("неизвестно, откуда удалён").to_string())?;
    let dir = orig.parent().map(|p| p.to_path_buf()).unwrap_or_else(crate::paths::home);
    let _ = std::fs::create_dir_all(&dir);
    let dst = crate::fsx::free_name(&dir, &orig.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    std::fs::rename(&e.path, &dst).map_err(|er| crate::fsx::err_text(&er))?;
    let _ = std::fs::remove_file(info_of(&e.path));
    Ok(dst)
}

pub fn forget(p: &Path) {
    let _ = std::fs::remove_file(info_of(p));
}

/// в корзине ли путь (файлы прямо в */Trash/files или .Trash-uid/files)
pub fn is_trashed(p: &Path) -> bool {
    p.parent().map(|f| f.file_name().map(|n| n == "files").unwrap_or(false) && f.parent().map(|t| t == home_trash() || t.file_name().map(|n| n.to_string_lossy().starts_with(".Trash")).unwrap_or(false)).unwrap_or(false)).unwrap_or(false)
}

pub fn all_files() -> Vec<PathBuf> {
    trash_dirs().iter().flat_map(|(t, _)| std::fs::read_dir(t.join("files")).map(|r| r.flatten().map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default()).collect()
}

#[cfg(test)]
mod tests {
    use super::orig_of;
    use std::path::Path;

    /// .trashinfo с чужого диска возвращает файл только на этот диск
    #[test]
    fn a_foreign_trashinfo_stays_on_its_disk() {
        let top = Path::new("/run/media/u/STICK");
        let at = |v: &str| orig_of(Some(top), v);
        assert_eq!(at("photos/a%20b.jpg").as_deref(), Some(Path::new("/run/media/u/STICK/photos/a b.jpg")));
        // абсолютный путь на этом же диске (так пишут некоторые программы) — годится
        assert_eq!(at("/run/media/u/STICK/photos/a.jpg").as_deref(), Some(Path::new("/run/media/u/STICK/photos/a.jpg")));
        // всё, что ведёт с диска прочь: вверх, абсолютным путём, через корень диска и вверх
        for v in ["../../.bashrc", "/home/u/.config/autostart/x.desktop", "/run/media/u/STICK/../../../../home/u/.profile", "/run/media/u/STICKY/a", "", "/run/media/u/STICK"] {
            assert_eq!(at(v), None, "{v}");
        }
        // домашняя корзина — своя: путь как записан
        assert_eq!(orig_of(None, "/home/u/Documents/a.txt").as_deref(), Some(Path::new("/home/u/Documents/a.txt")));
    }
}
