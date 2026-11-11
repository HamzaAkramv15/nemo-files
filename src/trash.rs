// Корзина по спецификации freedesktop: ~/.local/share/Trash для домашнего диска,
// <точка монтирования>/.Trash-<uid> для остальных. Общая с Dolphin, Nautilus, gio trash.

use crate::fsx::Entry;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

pub fn home_trash() -> PathBuf {
    crate::paths::data_dir().join("Trash")
}

/// точка монтирования, на которой лежит p
fn topdir(p: &Path) -> Option<PathBuf> {
    let dev = std::fs::symlink_metadata(p).ok()?.dev();
    let mut cur = p.to_path_buf();
    loop {
        let parent = cur.parent()?.to_path_buf();
        if std::fs::metadata(&parent).ok()?.dev() != dev {
            return Some(cur);
        }
        if parent == Path::new("/") {
            return Some(parent);
        }
        cur = parent;
    }
}

/// настоящая папка (не ссылка) и наша
fn own_dir(d: &Path) -> bool {
    std::fs::symlink_metadata(d).is_ok_and(|m| m.is_dir() && m.uid() == uid())
}

/// корзина на чужом диске — настоящая: сама .Trash-<uid> и её files — наши папки, не ссылки; info —
/// тоже, если есть. Диск приносят со стороны: files, сделанная ссылкой на /home, показала бы домашние
/// папки «файлами в корзине», и «очистить корзину» удалила бы их на самом деле. Та же проверка у glib
fn real_trash(t: &Path) -> bool {
    let info = t.join("info");
    own_dir(t) && own_dir(&t.join("files")) && (std::fs::symlink_metadata(&info).is_err() || own_dir(&info))
}

fn trash_for(p: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let ht = home_trash();
    let _ = std::fs::create_dir_all(&ht);
    let hdev = std::fs::metadata(crate::paths::data_dir()).ok()?.dev();
    let pdev = std::fs::symlink_metadata(p).ok()?.dev();
    if hdev == pdev {
        return Some((ht, None));
    }
    let top = topdir(p)?;
    let t = top.join(format!(".Trash-{}", uid()));
    // под этим именем уже что-то не наше (ссылка, чужая папка): туда не кладём и ничего там не создаём
    if std::fs::symlink_metadata(&t).is_ok() && !own_dir(&t) {
        return None;
    }
    // своя корзина на общем диске — только своя (0700, как велит спецификация): удалённое не читают другие
    {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&t);
    }
    if std::fs::create_dir_all(t.join("files")).is_ok() && std::fs::create_dir_all(t.join("info")).is_ok() && real_trash(&t) {
        return Some((t, Some(top)));
    }
    None
}

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

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn dec(s: &str) -> PathBuf {
    crate::icons::uri_path(&format!("file://{}", if s.starts_with('/') { s.to_string() } else { format!("/{s}") })).unwrap_or_else(|| PathBuf::from(s))
}

fn stamp() -> String {
    let t = crate::fsx::local_tm(crate::fsx::now());
    let mut s: libc::tm = unsafe { std::mem::zeroed() };
    let now = crate::fsx::now() as _;
    unsafe { libc::localtime_r(&now, &mut s) };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year, t.mon, t.day, t.hour, t.min, s.tm_sec)
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
    let (t, top) = trash_for(p).ok_or_else(|| crate::i18n::tr("корзина недоступна").to_string())?;
    let files = t.join("files");
    let info = t.join("info");
    let _ = std::fs::create_dir_all(&files);
    let _ = std::fs::create_dir_all(&info);
    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    // имя в корзине: занятое — «имя (2)»; .trashinfo создаётся первым и атомарно (O_EXCL)
    let mut target = files.join(&name);
    let mut n = 1;
    let infofile = loop {
        let tn = target.file_name().unwrap().to_string_lossy().to_string();
        let ip = info.join(format!("{tn}.trashinfo"));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&ip) {
            Ok(mut f) => {
                use std::io::Write;
                let shown = match &top {
                    Some(top) => p.strip_prefix(top).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string_lossy().to_string()),
                    None => p.to_string_lossy().to_string(),
                };
                let _ = write!(f, "[Trash Info]\nPath={}\nDeletionDate={}\n", enc(&shown), stamp());
                break ip;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                n += 1;
                let (stem, ext) = match name.rfind('.') {
                    Some(i) if i > 0 => (&name[..i], &name[i..]),
                    _ => (name.as_str(), ""),
                };
                target = files.join(format!("{stem} ({n}){ext}"));
            }
            Err(e) => return Err(crate::fsx::err_text(&e)),
        }
    };
    if let Err(e) = std::fs::rename(p, &target) {
        let _ = std::fs::remove_file(&infofile);
        return Err(crate::fsx::err_text(&e));
    }
    Ok(target)
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
    use super::{orig_of, real_trash};
    use std::path::Path;

    /// корзина на принесённом диске, собранная из ссылок, — не корзина: то, на что она указывает, не
    /// показывается «файлами в корзине» и не удаляется её очисткой
    #[test]
    fn a_trash_made_of_links_is_not_a_trash() {
        let d = crate::paths::private_tmp("trash-test").unwrap();
        let home = d.join("home");
        std::fs::create_dir_all(home.join("alice/Documents")).unwrap();
        let mk = |disk: &str| {
            let t = d.join(disk).join(".Trash-1000");
            std::fs::create_dir_all(&t).unwrap();
            t
        };
        // как её делает любой файловый менеджер
        let good = mk("good");
        std::fs::create_dir_all(good.join("files")).unwrap();
        std::fs::create_dir_all(good.join("info")).unwrap();
        assert!(real_trash(&good));
        // без info (ещё ничего не удаляли) — тоже корзина
        let fresh = mk("fresh");
        std::fs::create_dir_all(fresh.join("files")).unwrap();
        assert!(real_trash(&fresh));
        // files — ссылка на /home: «в корзине» оказались бы домашние папки
        let bait = mk("bait");
        std::fs::create_dir_all(bait.join("info")).unwrap();
        std::os::unix::fs::symlink(&home, bait.join("files")).unwrap();
        assert!(bait.join("files/alice").is_dir(), "the link does lead to the home folders");
        assert!(!real_trash(&bait));
        // сама .Trash-<uid> — ссылка
        std::fs::create_dir_all(d.join("linked")).unwrap();
        std::os::unix::fs::symlink(&good, d.join("linked/.Trash-1000")).unwrap();
        assert!(!real_trash(&d.join("linked/.Trash-1000")));
        // info — ссылка
        let info = mk("info");
        std::fs::create_dir_all(info.join("files")).unwrap();
        std::os::unix::fs::symlink(&home, info.join("info")).unwrap();
        assert!(!real_trash(&info));
        // файл вместо папки, и вовсе ничего
        let file = mk("file");
        std::fs::write(file.join("files"), b"").unwrap();
        assert!(!real_trash(&file) && !real_trash(&d.join("nothing/.Trash-1000")));
        assert!(home.join("alice/Documents").is_dir());
        let _ = std::fs::remove_dir_all(&d);
    }

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
