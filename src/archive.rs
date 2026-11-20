// Что внутри архива, не распаковывая: в панели сведений у выбранного архива — верхний уровень содержимого и сколько всего
// записей. Список читает `bsdtar -tf` в отдельной нити (большой сжатый архив не должен вешать окно), первые тысячи строк;
// панель перерисовывается, пока список не придёт. Одно место в кэше — последний выбранный архив.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// сколько строк списка читаем, дальше «и ещё»
const MAX_LINES: usize = 5000;

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// верхний уровень: имя и папка ли это
    pub top: Vec<(String, bool)>,
    /// записей в прочитанной части
    pub total: usize,
    /// список длиннее, чем прочитано
    pub more: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prev {
    Loading,
    Done(Summary),
    Failed(String),
}

/// строки bsdtar → верхний уровень (первый компонент пути, в порядке появления) и число записей
pub fn summarize(lines: &[String], more: bool) -> Summary {
    let mut top: Vec<(String, bool)> = vec![];
    for l in lines {
        let l = l.trim_start_matches("./");
        if l.trim().is_empty() {
            continue;
        }
        let (first, rest) = match l.split_once('/') {
            Some((a, b)) => (a, Some(b)),
            None => (l, None),
        };
        if first.is_empty() {
            continue;
        }
        // есть что-то после первого компонента — это папка (даже если её запись в архиве не перечислена)
        let is_dir = rest.is_some();
        match top.iter_mut().find(|(n, _)| n == first) {
            Some(t) => t.1 |= is_dir,
            None => top.push((first.to_string(), is_dir)),
        }
    }
    Summary { top, total: lines.iter().filter(|l| !l.trim().is_empty()).count(), more }
}

static CACHE: Mutex<Option<(PathBuf, Prev)>> = Mutex::new(None);

/// состояние предпросмотра этого архива; первый вызов запускает чтение в нити
pub fn get(path: &Path) -> Prev {
    let Ok(mut c) = CACHE.lock() else { return Prev::Failed(String::new()) };
    if let Some((p, prev)) = c.as_ref() {
        if p == path {
            return prev.clone();
        }
    }
    *c = Some((path.to_path_buf(), Prev::Loading));
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        let r = read(&p);
        if let Ok(mut c) = CACHE.lock() {
            if c.as_ref().is_some_and(|(cp, _)| *cp == p) {
                *c = Some((p, r));
            }
        }
    });
    Prev::Loading
}

fn read(p: &Path) -> Prev {
    let Ok(mut child) = Command::new("bsdtar").arg("-tf").arg(p).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
        return Prev::Failed(crate::tf!("bsdtar не найден", "bsdtar not found", "bsdtar não encontrado"));
    };
    let out = child.stdout.take().expect("piped");
    let mut lines = vec![];
    let mut more = false;
    for l in BufReader::new(out).lines().map_while(Result::ok) {
        if lines.len() >= MAX_LINES {
            more = true;
            break;
        }
        lines.push(l);
    }
    if more {
        // дальше читать незачем: большой архив не распаковываем до конца ради предпросмотра
        let _ = child.kill();
    }
    let status = child.wait();
    if lines.is_empty() && !status.is_ok_and(|s| s.success()) {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut e, &mut err);
        }
        return Prev::Failed(err.lines().next().unwrap_or("").chars().take(80).collect());
    }
    Prev::Done(summarize(&lines, more))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_top_level_is_the_first_path_component_in_order() {
        let s = summarize(&l(&["proj/", "proj/src/main.rs", "proj/README.md", "notes.txt", "./other/x"]), false);
        assert_eq!(s.top, vec![("proj".to_string(), true), ("notes.txt".to_string(), false), ("other".to_string(), true)]);
        assert_eq!(s.total, 5);
        assert!(!s.more);
    }

    #[test]
    fn a_folder_missing_from_the_list_is_still_a_folder() {
        let s = summarize(&l(&["a/b/c.txt"]), true);
        assert_eq!(s.top, vec![("a".to_string(), true)]);
        assert!(s.more);
    }

    #[test]
    fn blank_lines_are_skipped() {
        let s = summarize(&l(&["", "x", "  "]), false);
        assert_eq!((s.top.len(), s.total), (1, 1));
    }

    /// настоящий архив: список приходит из bsdtar (пропускается, если его нет)
    #[test]
    fn a_real_archive_is_listed_without_blocking() {
        if nemo_px::paths::find_exe("bsdtar").is_none() {
            return;
        }
        let d = nemo_px::paths::private_tmp("archive-test").unwrap();
        std::fs::create_dir_all(d.join("src/inner")).unwrap();
        std::fs::write(d.join("src/inner/a.txt"), "a").unwrap();
        std::fs::write(d.join("src/b.txt"), "b").unwrap();
        let out = Command::new("bsdtar").current_dir(&d).args(["-cf", "t.tar", "src"]).output().unwrap();
        assert!(out.status.success());
        let p = d.join("t.tar");
        let mut got = Prev::Loading;
        for _ in 0..100 {
            got = get(&p);
            if got != Prev::Loading {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        match got {
            Prev::Done(s) => {
                assert_eq!(s.top, vec![("src".to_string(), true)]);
                assert_eq!(s.total, 4, "src/, src/inner/, a.txt and b.txt");
            }
            other => panic!("{other:?}"),
        }
        // не архив — понятная ошибка, а не пустота
        std::fs::write(d.join("bad.tar"), "not an archive at all").unwrap();
        let mut bad = Prev::Loading;
        for _ in 0..100 {
            bad = get(&d.join("bad.tar"));
            if bad != Prev::Loading {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(matches!(bad, Prev::Failed(_)), "{bad:?}");
        let _ = std::fs::remove_dir_all(d);
    }
}
