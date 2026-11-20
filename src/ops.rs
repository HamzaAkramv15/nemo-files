// Операции с файлами в отдельной нити: копирование и перенос с прогрессом, корзина, удаление,
// ссылки, архивы (bsdtar). Конфликт имён — вопрос в окне, нить ждёт ответа.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Copy,
    Move,
    Trash,
    Delete,
    Link,
    Extract,
    Compress,
    Restore,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolve {
    Skip,
    Replace,
    Rename,
    Cancel,
}

pub enum Ev {
    Progress { id: u32, done: u64, total: u64, name: String },
    Conflict { src: PathBuf, dst: PathBuf, reply: mpsc::Sender<(Resolve, bool)> },
    /// pairs: (откуда, куда) — для отмены и подсветки нового; merged — папки, слитые с уже бывшими там
    /// («заменить» папку папкой): отмена не трогает их, в них лежало и чужое
    Done { id: u32, kind: Kind, errors: Vec<String>, pairs: Vec<(PathBuf, PathBuf)>, merged: Vec<PathBuf> },
}

pub struct Job {
    pub id: u32,
    pub kind: Kind,
    pub srcs: Vec<PathBuf>,
    pub dst: PathBuf,
    /// для сжатия: имя архива
    pub name: String,
    pub stop: Arc<AtomicBool>,
    /// пауза: пока стоит, копирование и перенос не идут дальше (отмена всё равно срабатывает)
    pub pause: Arc<AtomicBool>,
}

pub fn spawn(job: Job, send: impl Fn(Ev) + Send + 'static) {
    std::thread::spawn(move || run(job, &send));
}

struct Ctx<'a> {
    id: u32,
    send: &'a dyn Fn(Ev),
    done: u64,
    total: u64,
    last: Instant,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    /// «для всех»: ответ на следующие конфликты
    all: Option<Resolve>,
    errors: Vec<String>,
    /// замена файла: (временное имя, настоящее). Копия идёт во временное, и только целая встаёт на место
    swap: Option<(PathBuf, PathBuf)>,
}

impl Ctx<'_> {
    fn tick(&mut self, name: &str, force: bool) {
        if force || self.last.elapsed().as_millis() > 60 {
            self.last = Instant::now();
            (self.send)(Ev::Progress { id: self.id, done: self.done, total: self.total, name: name.to_string() });
        }
    }
    /// остановлено ли дело; на паузе ждёт здесь же (копирование зовёт это на каждом куске, так что встаёт сразу), отмена будит
    fn stopped(&self) -> bool {
        while self.pause.load(Ordering::Relaxed) && !self.stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        self.stop.load(Ordering::Relaxed)
    }
    fn err(&mut self, p: &Path, e: &std::io::Error) {
        self.errors.push(format!("{}: {}", p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), crate::fsx::err_text(e)));
    }
    /// куда класть src в dir: None — пропустить
    fn target(&mut self, src: &Path, dir: &Path) -> Option<PathBuf> {
        let name = src.file_name()?.to_string_lossy().to_string();
        let dst = dir.join(&name);
        if std::fs::symlink_metadata(&dst).is_err() {
            return Some(dst);
        }
        // копия в ту же папку — сразу «имя (2)»
        if src.parent() == Some(dir) {
            return Some(crate::fsx::free_name(dir, &name));
        }
        let r = match self.all {
            Some(r) => r,
            None => {
                let (tx, rx) = mpsc::channel();
                (self.send)(Ev::Conflict { src: src.to_path_buf(), dst: dst.clone(), reply: tx });
                let (r, all) = rx.recv().unwrap_or((Resolve::Cancel, false));
                if all {
                    self.all = Some(r);
                }
                r
            }
        };
        match r {
            Resolve::Skip => None,
            Resolve::Cancel => {
                self.stop.store(true, Ordering::Relaxed);
                None
            }
            Resolve::Rename => Some(crate::fsx::free_name(dir, &name)),
            Resolve::Replace => {
                if same(src, &dst) {
                    return None;
                }
                let m = std::fs::symlink_metadata(&dst);
                // файл поверх файла: старый не трогаем, пока новый не скопирован целиком
                if !src.is_dir() && m.as_ref().is_ok_and(|m| !m.is_dir()) {
                    let tmp = temp_name(dir, &name);
                    self.swap = Some((tmp.clone(), dst));
                    return Some(tmp);
                }
                let gone = if m.map(|m| m.is_dir()).unwrap_or(false) && !src.is_dir() { std::fs::remove_dir_all(&dst) } else if src.is_dir() && dst.is_dir() { Ok(()) } else { std::fs::remove_file(&dst) };
                if let Err(e) = gone {
                    self.err(&dst, &e);
                    return None;
                }
                Some(dst)
            }
        }
    }
}

/// кладут ли папку внутрь неё самой: по настоящим путям, чтобы ссылка на папку не обманула проверку
/// (иначе копирование шло бы, пока путь не станет слишком длинным)
fn into_itself(src: &Path, dst: &Path) -> bool {
    if !src.is_dir() {
        return false;
    }
    match (std::fs::canonicalize(src), std::fs::canonicalize(dst)) {
        (Ok(s), Ok(d)) => d.starts_with(s),
        _ => dst.starts_with(src),
    }
}

/// скрытое временное имя рядом с настоящим (та же папка — значит, тот же диск, и rename атомарен)
fn temp_name(dir: &Path, name: &str) -> PathBuf {
    let stem: String = name.chars().take(120).collect();
    dir.join(format!(".{stem}.nemo-part-{}", std::process::id()))
}

/// довести замену: целая копия встаёт на место старого файла одним rename; иначе временное убирается,
/// а старый остаётся как был. Возвращает путь, под которым лежит результат
fn settle(c: &mut Ctx, swap: Option<(PathBuf, PathBuf)>, ok: bool, d: &Path) -> PathBuf {
    let Some((tmp, fin)) = swap else { return d.to_path_buf() };
    if ok {
        // замена не стирает старый файл: он уходит в корзину и его можно вернуть. Корзины на этом диске нет (телефон,
        // сеть) — заменяем, как человек и выбрал в окне; корзина есть, но не приняла файл — не заменяем вовсе
        if std::fs::symlink_metadata(&fin).is_ok() {
            if let Err(nemo_px::trash::PutErr::Io(e)) = nemo_px::trash::put(&fin) {
                c.errors.push(format!("{}: {}", fin.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), crate::tf!("прежний файл не удалось убрать в корзину, замены нет: {}", "the old file could not go to the trash, not replaced: {}", "o arquivo antigo não foi para a lixeira, não substituído: {}", crate::fsx::err_text(&e))));
                let _ = std::fs::remove_file(&tmp);
                return fin;
            }
        }
        match std::fs::rename(&tmp, &fin) {
            Ok(()) => return fin,
            Err(e) => c.err(&fin, &e),
        }
    }
    let _ = std::fs::remove_file(&tmp);
    fin
}

fn same(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn size_of(p: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else { return 0 };
    if !m.is_dir() {
        return m.len();
    }
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            n += size_of(&e.path());
        }
    }
    n
}

fn copy_file(c: &mut Ctx, src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    let mut a = std::fs::File::open(src)?;
    let meta = a.metadata()?;
    let mut b = std::fs::File::create(dst)?;
    let name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    // копирует ядро (copy_file_range): на btrfs и xfs внутри одного диска это reflink — мгновенно
    // и без лишнего места, между дисками — без прохода данных через память программы. Кусками:
    // прогресс и отмена. Не умеет (FUSE — телефоны и gvfs, старое ядро между ФС, «файлы» /proc
    // с нулевым размером) — обычное чтение и запись с того же места
    let mut kernel = true;
    let mut copied = 0u64;
    let mut buf: Option<Vec<u8>> = None;
    loop {
        if c.stopped() {
            drop(b);
            let _ = std::fs::remove_file(dst);
            return Ok(());
        }
        let n = if kernel {
            let n = unsafe { libc::copy_file_range(a.as_raw_fd(), std::ptr::null_mut(), b.as_raw_fd(), std::ptr::null_mut(), 16 << 20, 0) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                if copied == 0 && matches!(e.raw_os_error(), Some(libc::EXDEV | libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP | libc::EPERM | libc::EBADF)) {
                    kernel = false;
                    continue;
                }
                return Err(e);
            }
            if n == 0 && copied == 0 {
                // пустой — или псевдофайл, у которого размер 0, а данные есть: пусть решит read
                kernel = false;
                continue;
            }
            n as usize
        } else {
            let buf = buf.get_or_insert_with(|| vec![0u8; 1 << 20]);
            let n = a.read(buf)?;
            b.write_all(&buf[..n])?;
            n
        };
        if n == 0 {
            break;
        }
        copied += n as u64;
        c.done += n as u64;
        c.tick(&name, false);
    }
    let _ = b.set_permissions(meta.permissions());
    if let Ok(t) = meta.modified() {
        let _ = b.set_modified(t);
    }
    Ok(())
}

fn copy_tree(c: &mut Ctx, src: &Path, dst: &Path) {
    if c.stopped() {
        return;
    }
    let Ok(m) = std::fs::symlink_metadata(src) else { return };
    if m.file_type().is_symlink() {
        if let Ok(t) = std::fs::read_link(src) {
            if let Err(e) = std::os::unix::fs::symlink(t, dst) {
                c.err(src, &e);
            }
        }
    } else if m.is_dir() {
        if let Err(e) = std::fs::create_dir(dst) {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                c.err(src, &e);
                return;
            }
        }
        if let Ok(rd) = std::fs::read_dir(src) {
            for e in rd.flatten() {
                copy_tree(c, &e.path(), &dst.join(e.file_name()));
            }
        }
        let _ = std::fs::set_permissions(dst, m.permissions());
    } else if !m.is_file() {
        // канал (FIFO) — новый такой же, а не чтение из него (оно ждало бы писателя вечно, и отмена
        // не помогла бы); устройство или сокет не копируются — их содержимое не файл
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        if m.file_type().is_fifo() {
            use std::os::unix::ffi::OsStrExt;
            // SAFETY: mkfifo с путём, оканчивающимся нулём (CString)
            let made = std::ffi::CString::new(dst.as_os_str().as_bytes()).is_ok_and(|p| unsafe { libc::mkfifo(p.as_ptr(), m.permissions().mode() & 0o7777) } == 0);
            if !made {
                c.err(src, &std::io::Error::last_os_error());
            }
        } else {
            c.err(src, &std::io::Error::from(std::io::ErrorKind::Unsupported));
        }
    } else if let Err(e) = copy_file(c, src, dst) {
        c.err(src, &e);
    }
}

fn run(job: Job, send: &dyn Fn(Ev)) {
    let mut c = Ctx { id: job.id, send, done: 0, total: 0, last: Instant::now(), stop: job.stop.clone(), pause: job.pause.clone(), all: None, errors: vec![], swap: None };
    let mut pairs: Vec<(PathBuf, PathBuf)> = vec![];
    let mut merged: Vec<PathBuf> = vec![];
    match job.kind {
        Kind::Copy | Kind::Move => {
            c.total = job.srcs.iter().map(|s| size_of(s)).sum();
            c.tick("", true);
            for s in &job.srcs {
                if c.stopped() {
                    break;
                }
                if into_itself(s, &job.dst) {
                    c.errors.push(crate::tf!("{}: нельзя положить папку в саму себя", "{}: cannot put a folder inside itself", "{}: não é possível colocar uma pasta dentro dela mesma", s.display()));
                    continue;
                }
                if job.kind == Kind::Move && s.parent() == Some(job.dst.as_path()) {
                    continue;
                }
                let Some(d) = c.target(s, &job.dst) else { continue };
                let swap = c.swap.take();
                if job.kind == Kind::Move {
                    match std::fs::rename(s, &d) {
                        Ok(()) => {
                            c.done += size_of(&d);
                            let d = settle(&mut c, swap, true, &d);
                            pairs.push((s.clone(), d));
                            continue;
                        }
                        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
                        Err(e) => {
                            c.err(s, &e);
                            settle(&mut c, swap, false, &d);
                            continue;
                        }
                    }
                }
                let before = c.errors.len();
                if s.is_dir() && !s.is_symlink() && std::fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) {
                    merged.push(d.clone());
                }
                copy_tree(&mut c, s, &d);
                if c.stopped() {
                    settle(&mut c, swap, false, &d);
                    break;
                }
                let ok = c.errors.len() == before;
                let d = settle(&mut c, swap, ok, &d);
                if job.kind == Kind::Move && c.errors.len() == before {
                    let r = if s.is_dir() && !s.is_symlink() { std::fs::remove_dir_all(s) } else { std::fs::remove_file(s) };
                    if let Err(e) = r {
                        c.err(s, &e);
                    }
                }
                pairs.push((s.clone(), d));
            }
        }
        Kind::Trash => {
            c.total = job.srcs.len() as u64;
            for s in &job.srcs {
                match crate::trash::put(s) {
                    Ok(t) => pairs.push((s.clone(), t)),
                    Err(e) => c.errors.push(format!("{}: {e}", s.display())),
                }
                c.done += 1;
                c.tick(&s.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(), false);
            }
        }
        Kind::Delete => {
            c.total = job.srcs.len() as u64;
            for s in &job.srcs {
                let r = if s.is_dir() && !s.is_symlink() { std::fs::remove_dir_all(s) } else { std::fs::remove_file(s) };
                match r {
                    Ok(()) => {
                        // удалили из корзины — её запись о файле тоже; у обычного файла рядом ничего своего нет
                        if crate::trash::is_trashed(s) {
                            crate::trash::forget(s);
                        }
                        pairs.push((s.clone(), PathBuf::new()));
                    }
                    Err(e) => c.err(s, &e),
                }
                c.done += 1;
                c.tick("", false);
            }
        }
        Kind::Restore => {
            for s in &job.srcs {
                if let Some(e) = crate::fsx::Entry::from_path(s) {
                    let mut e = e;
                    // путь оригинала — из .trashinfo
                    if let Some(t) = crate::trash::list().into_iter().find(|t| t.path == *s) {
                        e.orig = t.orig;
                    }
                    match crate::trash::restore(&e) {
                        Ok(d) => pairs.push((s.clone(), d)),
                        Err(er) => c.errors.push(format!("{}: {er}", e.name)),
                    }
                }
            }
        }
        Kind::Link => {
            for s in &job.srcs {
                let name = s.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                let d = crate::fsx::free_name(&job.dst, &name);
                match std::os::unix::fs::symlink(s, &d) {
                    Ok(()) => pairs.push((s.clone(), d)),
                    Err(e) => c.err(s, &e),
                }
            }
        }
        Kind::Extract => {
            for s in &job.srcs {
                let name = s.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                let stem: String = name.chars().take(crate::fsx::stem_len(&name, false)).collect();
                let d = crate::fsx::free_name(&job.dst, &stem);
                if let Err(e) = std::fs::create_dir_all(&d) {
                    c.err(s, &e);
                    continue;
                }
                c.tick(&name, true);
                let ok = std::process::Command::new("bsdtar").arg("-xf").arg(s).arg("-C").arg(&d).status().map(|st| st.success()).unwrap_or(false);
                if ok {
                    // единственная папка внутри — поднять на уровень вверх
                    let inner: Vec<PathBuf> = std::fs::read_dir(&d).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
                    if inner.len() == 1 && inner[0].is_dir() {
                        let up = crate::fsx::free_name(&job.dst, &inner[0].file_name().unwrap().to_string_lossy());
                        if std::fs::rename(&inner[0], &up).is_ok() {
                            let _ = std::fs::remove_dir(&d);
                            pairs.push((s.clone(), up));
                            continue;
                        }
                    }
                    pairs.push((s.clone(), d));
                } else {
                    let _ = std::fs::remove_dir(&d);
                    c.errors.push(crate::tf!("{}: не удалось распаковать", "{}: could not extract", "{}: não foi possível extrair", name));
                }
            }
        }
        Kind::Compress => {
            let out = crate::fsx::free_name(&job.dst, &job.name);
            c.tick(&job.name, true);
            let mut cmd = std::process::Command::new("bsdtar");
            // «--»: файл с именем «--use-compress-program=…» не станет ключом bsdtar
            cmd.arg("-a").arg("-cf").arg(&out).arg("-C").arg(&job.dst).arg("--");
            for s in &job.srcs {
                cmd.arg(s.strip_prefix(&job.dst).unwrap_or(s));
            }
            if cmd.status().map(|st| st.success()).unwrap_or(false) {
                pairs.push((job.dst.clone(), out));
            } else {
                let _ = std::fs::remove_file(&out);
                c.errors.push(crate::tf!("{}: не удалось сжать", "{}: could not compress", "{}: não foi possível compactar", job.name));
            }
        }
    }
    c.tick("", true);
    send(Ev::Done { id: job.id, kind: job.kind, errors: c.errors, pairs, merged });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(send: &'a dyn Fn(Ev)) -> Ctx<'a> {
        Ctx { id: 0, send, done: 0, total: 0, last: Instant::now(), stop: Arc::new(AtomicBool::new(false)), pause: Arc::new(AtomicBool::new(false)), all: None, errors: vec![], swap: None }
    }

    #[test]
    fn copies_by_kernel_and_by_hand() {
        let dir = nemo_px::paths::private_tmp("ops-test").expect("tmp");
        let big: Vec<u8> = (0..(40u32 << 20) / 4).flat_map(|i| i.to_le_bytes()).collect();
        let cases: Vec<(PathBuf, Vec<u8>)> = vec![
            (dir.join("big"), big),
            (dir.join("empty"), vec![]),
            (dir.join("small"), b"nemo".to_vec()),
        ];
        let send = |_: Ev| {};
        for (p, data) in &cases {
            std::fs::write(p, data).unwrap();
            let to = p.with_extension("copy");
            let mut c = ctx(&send);
            copy_file(&mut c, p, &to).unwrap();
            assert_eq!(&std::fs::read(&to).unwrap(), data);
            assert_eq!(c.done, data.len() as u64);
        }
        // размер 0, а данные есть: копия не пустая
        let to = dir.join("status");
        let mut c = ctx(&send);
        copy_file(&mut c, Path::new("/proc/self/status"), &to).unwrap();
        assert!(std::fs::read_to_string(&to).unwrap().contains("Name:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_cannot_go_into_itself_even_through_a_link() {
        let dir = nemo_px::paths::private_tmp("ops-self").expect("tmp");
        let a = dir.join("a");
        std::fs::create_dir_all(a.join("sub")).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert!(into_itself(&a, &a));
        assert!(into_itself(&a, &a.join("sub")));
        assert!(into_itself(&a, &link.join("sub")), "a link to the folder is the folder");
        assert!(!into_itself(&a, &dir));
        // файл — не папка; сосед с общим началом имени — не внутри
        std::fs::write(dir.join("f"), b"x").unwrap();
        assert!(!into_itself(&dir.join("f"), &dir));
        std::fs::create_dir(dir.join("ab")).unwrap();
        assert!(!into_itself(&a, &dir.join("ab")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// копия папки с вложенным, ссылкой, пустым файлом и каналом; права сохраняются
    #[test]
    fn copy_tree_keeps_structure_links_and_modes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = nemo_px::paths::private_tmp("ops-tree").expect("tmp");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("d/e")).unwrap();
        std::fs::write(src.join("d/e/f.txt"), b"hello").unwrap();
        std::fs::write(src.join("empty"), b"").unwrap();
        std::os::unix::fs::symlink("d/e/f.txt", src.join("ln")).unwrap();
        std::fs::set_permissions(src.join("d/e/f.txt"), std::fs::Permissions::from_mode(0o640)).unwrap();
        let send = |_: Ev| {};
        let mut c = ctx(&send);
        let dst = dir.join("dst");
        copy_tree(&mut c, &src, &dst);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert_eq!(std::fs::read(dst.join("d/e/f.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::metadata(dst.join("d/e/f.txt")).unwrap().permissions().mode() & 0o777, 0o640);
        assert!(std::fs::metadata(dst.join("empty")).unwrap().len() == 0);
        assert_eq!(std::fs::read_link(dst.join("ln")).unwrap(), Path::new("d/e/f.txt"), "the link stays a link");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// замена файла: пока копия не целая, старый на месте; целая встаёт одним rename, временных следов нет
    #[test]
    fn replacing_a_file_never_loses_the_old_one() {
        let dir = nemo_px::paths::private_tmp("ops-swap").expect("tmp");
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("f"), b"new").unwrap();
        std::fs::write(b.join("f"), b"old").unwrap();
        let send = |_: Ev| {};
        // успех: новое содержимое, ни временных файлов, ни потерь
        let mut c = ctx(&send);
        c.all = Some(Resolve::Replace);
        let d = c.target(&a.join("f"), &b).unwrap();
        assert_ne!(d, b.join("f"), "the copy goes to a temporary name first");
        assert_eq!(std::fs::read(b.join("f")).unwrap(), b"old", "old file still there while copying");
        let swap = c.swap.take();
        copy_tree(&mut c, &a.join("f"), &d);
        let fin = settle(&mut c, swap, true, &d);
        assert_eq!(fin, b.join("f"));
        assert_eq!(std::fs::read(b.join("f")).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&b).unwrap().count(), 1, "no leftovers");
        // неудача: старое остаётся как было, временное убрано
        std::fs::write(a.join("f"), b"newer").unwrap();
        let mut c = ctx(&send);
        c.all = Some(Resolve::Replace);
        let d = c.target(&a.join("f"), &b).unwrap();
        let swap = c.swap.take();
        std::fs::write(&d, b"half").unwrap();
        settle(&mut c, swap, false, &d);
        assert_eq!(std::fs::read(b.join("f")).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&b).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// замена файла файлом: прежний уходит в корзину и лежит там целым, новый встаёт на его место
    #[test]
    fn a_replaced_file_goes_to_the_trash_whole() {
        let dir = nemo_px::paths::private_tmp("ops-replace").expect("tmp");
        // своя корзина: XDG_DATA_HOME на время проверки (другие проверки этого крейта не читают корзину)
        let old_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", dir.join("share"));
        let (fin, tmp) = (dir.join("doc.txt"), dir.join(".doc.txt.nemo-part-1"));
        std::fs::write(&fin, "old contents").unwrap();
        std::fs::write(&tmp, "new contents").unwrap();
        let send = |_: Ev| {};
        let mut c = ctx(&send);
        let r = settle(&mut c, Some((tmp.clone(), fin.clone())), true, &fin);
        assert_eq!(r, fin);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert_eq!(std::fs::read_to_string(&fin).unwrap(), "new contents");
        let files = dir.join("share/Trash/files");
        let kept: Vec<String> = std::fs::read_dir(&files).map(|d| d.flatten().map(|e| std::fs::read_to_string(e.path()).unwrap_or_default()).collect()).unwrap_or_default();
        assert_eq!(kept, vec!["old contents".to_string()], "the old file is in the trash, unchanged");
        match old_home {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// пауза останавливает копирование между кусками, снятие — продолжает, отмена будит даже стоящее на паузе
    #[test]
    fn a_paused_copy_stands_still_and_goes_on_when_resumed() {
        let dir = nemo_px::paths::private_tmp("ops-pause").expect("tmp");
        let src = dir.join("big");
        let dst = dir.join("copy");
        std::fs::write(&src, vec![7u8; 200 << 20]).unwrap();
        let pause = Arc::new(AtomicBool::new(true));
        let (s2, d2, p2) = (src.clone(), dst.clone(), pause.clone());
        let h = std::thread::spawn(move || {
            let send = |_: Ev| {};
            let mut c = ctx(&send);
            c.pause = p2;
            copy_file(&mut c, &s2, &d2).is_ok()
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        let at = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let later = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
        assert_eq!(at, later, "no progress while paused");
        assert!(later < 200 << 20, "not finished while paused");
        pause.store(false, Ordering::Relaxed);
        assert!(h.join().unwrap(), "finished after resuming");
        assert_eq!(std::fs::metadata(&dst).unwrap().len(), 200 << 20);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancel_wakes_a_paused_copy_and_removes_the_half_file() {
        let dir = nemo_px::paths::private_tmp("ops-pause-cancel").expect("tmp");
        let src = dir.join("big");
        let dst = dir.join("copy");
        std::fs::write(&src, vec![1u8; 64 << 20]).unwrap();
        let (pause, stop) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)));
        let (s2, d2, p2, st2) = (src.clone(), dst.clone(), pause.clone(), stop.clone());
        let h = std::thread::spawn(move || {
            let send = |_: Ev| {};
            let mut c = ctx(&send);
            c.pause = p2;
            c.stop = st2;
            let _ = copy_file(&mut c, &s2, &d2);
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        stop.store(true, Ordering::Relaxed);
        h.join().unwrap();
        assert!(!dst.exists(), "a cancelled copy leaves no half-written file");
        let _ = std::fs::remove_dir_all(dir);
    }
}

