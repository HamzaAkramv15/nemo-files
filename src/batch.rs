// Пакетное переименование: несколько выделенных файлов и F2. Правила — найти и заменить в имени, регистр, номера, приписка
// в начале и в конце; расширение не трогается. План (новые имена) строится сразу, пока человек печатает, проблемы видны в
// списке до нажатия; переименование идёт в два шага (через временные имена), так что можно поменять местами `a` и `b`,
// а при сбое всё возвращается назад.

use crate::fsx;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Case {
    Keep,
    Lower,
    Upper,
    Title,
}

impl Case {
    pub fn next(self) -> Case {
        match self {
            Case::Keep => Case::Lower,
            Case::Lower => Case::Upper,
            Case::Upper => Case::Title,
            Case::Title => Case::Keep,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Case::Keep => "как есть",
            Case::Lower => "строчные",
            Case::Upper => "ПРОПИСНЫЕ",
            Case::Title => "Каждое Слово",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Num {
    Off,
    /// «имя 01»
    After,
    /// «01» вместо имени
    Instead,
}

impl Num {
    pub fn next(self) -> Num {
        match self {
            Num::Off => Num::After,
            Num::After => Num::Instead,
            Num::Instead => Num::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Num::Off => "нет",
            Num::After => "после имени",
            Num::Instead => "вместо имени",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub find: String,
    pub replace: String,
    pub prefix: String,
    pub suffix: String,
    pub case: Case,
    pub num: Num,
}

impl Default for Rule {
    fn default() -> Rule {
        Rule { find: String::new(), replace: String::new(), prefix: String::new(), suffix: String::new(), case: Case::Keep, num: Num::Off }
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = matches!(c, ' ' | '_' | '-' | '.');
        }
    }
    out
}

/// новое имя для файла номер `idx` из `count`
pub fn rename_one(name: &str, dir: bool, idx: usize, count: usize, r: &Rule) -> String {
    let n = fsx::stem_len(name, dir);
    let stem: String = name.chars().take(n).collect();
    let ext: String = name.chars().skip(n).collect();
    let mut stem = if r.find.is_empty() { stem } else { stem.replace(&r.find, &r.replace) };
    stem = match r.case {
        Case::Keep => stem,
        Case::Lower => stem.to_lowercase(),
        Case::Upper => stem.to_uppercase(),
        Case::Title => title_case(&stem),
    };
    let width = count.to_string().len().max(2);
    let number = format!("{:0width$}", idx + 1);
    match r.num {
        Num::Off => {}
        Num::After => stem = if stem.is_empty() { number } else { format!("{stem} {number}") },
        Num::Instead => stem = number,
    }
    format!("{}{}{}{}", r.prefix, stem, r.suffix, ext)
}

pub fn plan(items: &[(String, bool)], r: &Rule) -> Vec<String> {
    items.iter().enumerate().map(|(i, (n, d))| rename_one(n, *d, i, items.len(), r)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Problem {
    Empty,
    Slash,
    /// два файла получили бы одно имя
    Duplicate,
    /// такое имя уже есть в папке
    Exists,
}

impl Problem {
    pub fn text(self) -> &'static str {
        match self {
            Problem::Empty => "имя пустое",
            Problem::Slash => "в имени не может быть «/»",
            Problem::Duplicate => "два файла получат одно имя",
            Problem::Exists => "такое имя уже есть",
        }
    }
}

/// что не так с каждым из новых имён; `exists` — есть ли в папке имя (файл не из выделенных)
pub fn check(old: &[String], new: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<Option<Problem>> {
    (0..old.len())
        .map(|i| {
            if new[i] == old[i] {
                return None;
            }
            let n = &new[i];
            if n.trim().is_empty() || n == "." || n == ".." {
                return Some(Problem::Empty);
            }
            if n.contains('/') {
                return Some(Problem::Slash);
            }
            if (0..new.len()).any(|j| j != i && new[j] == *n) {
                return Some(Problem::Duplicate);
            }
            // занято чужим файлом: тем, что не выделен (выделенные уходят со своих имён)
            if exists(n) && !old.contains(n) {
                return Some(Problem::Exists);
            }
            None
        })
        .collect()
}

/// план для окна: прежние имена, новые и что не так с каждым; `dir` — где лежат файлы (для проверки «имя уже есть»)
pub fn preview(items: &[(PathBuf, bool)], r: &Rule) -> (Vec<String>, Vec<String>, Vec<Option<Problem>>) {
    let named: Vec<(String, bool)> = items.iter().map(|(p, d)| (p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), *d)).collect();
    let old: Vec<String> = named.iter().map(|x| x.0.clone()).collect();
    let new = plan(&named, r);
    let dir = items.first().and_then(|(p, _)| p.parent()).map(Path::to_path_buf).unwrap_or_default();
    let probs = check(&old, &new, &|n| std::fs::symlink_metadata(dir.join(n)).is_ok());
    (old, new, probs)
}

/// переименовать в два шага; `rename` подставляется ради проверки сбоя. Ok — пары (было, стало) для отмены;
/// Err — что не вышло, всё уже возвращено
pub fn apply(dir: &Path, old: &[String], new: &[String], rename: &dyn Fn(&Path, &Path) -> io::Result<()>) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let changed: Vec<usize> = (0..old.len()).filter(|&i| old[i] != new[i]).collect();
    let tmp = |i: usize| dir.join(format!(".nemo-rename-{}-{i}", std::process::id()));
    let mut moved: Vec<usize> = vec![];
    // шаг 1: все меняемые уходят на временные имена: так ничто не наступает на чужое имя
    for &i in &changed {
        if let Err(e) = rename(&dir.join(&old[i]), &tmp(i)) {
            for &j in moved.iter().rev() {
                let _ = rename(&tmp(j), &dir.join(&old[j]));
            }
            return Err(format!("{}: {}", old[i], fsx::err_text(&e)));
        }
        moved.push(i);
    }
    // шаг 2: временные — на новые
    let mut done: Vec<usize> = vec![];
    for &i in &changed {
        if let Err(e) = rename(&tmp(i), &dir.join(&new[i])) {
            // назад: сделанные из новых, остальные из временных
            for &j in done.iter().rev() {
                let _ = rename(&dir.join(&new[j]), &tmp(j));
            }
            for &j in changed.iter().rev() {
                let _ = rename(&tmp(j), &dir.join(&old[j]));
            }
            return Err(format!("{}: {}", new[i], fsx::err_text(&e)));
        }
        done.push(i);
    }
    Ok(changed.iter().map(|&i| (dir.join(&old[i]), dir.join(&new[i]))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(v: &[&str]) -> Vec<(String, bool)> {
        v.iter().map(|s| (s.to_string(), false)).collect()
    }

    #[test]
    fn find_replace_changes_the_name_but_never_the_extension() {
        let r = Rule { find: "IMG".into(), replace: "photo".into(), ..Rule::default() };
        assert_eq!(plan(&items(&["IMG_001.jpg", "IMG.png", "notes.txt"]), &r), ["photo_001.jpg", "photo.png", "notes.txt"]);
        let r = Rule { find: "jpg".into(), replace: "X".into(), ..Rule::default() };
        assert_eq!(plan(&items(&["a.jpg"]), &r), ["a.jpg"], "the extension is not part of the name");
        let r = Rule { find: "a".into(), replace: "b".into(), ..Rule::default() };
        assert_eq!(plan(&items(&["a.tar.gz"]), &r), ["b.tar.gz"], "a double extension stays whole");
    }

    #[test]
    fn case_numbers_and_affixes_combine_in_a_fixed_order() {
        let r = Rule { case: Case::Upper, prefix: "x_".into(), suffix: "_y".into(), ..Rule::default() };
        assert_eq!(plan(&items(&["ab.txt"]), &r), ["x_AB_y.txt"]);
        let r = Rule { case: Case::Title, ..Rule::default() };
        assert_eq!(plan(&items(&["my holiday_photo-one.png"]), &r), ["My Holiday_Photo-One.png"]);
        let r = Rule { num: Num::After, ..Rule::default() };
        assert_eq!(plan(&items(&["a.txt", "b.txt"]), &r), ["a 01.txt", "b 02.txt"]);
        let r = Rule { num: Num::Instead, prefix: "scan-".into(), ..Rule::default() };
        assert_eq!(plan(&items(&["a.jpg", "b.jpg", "c.jpg"]), &r), ["scan-01.jpg", "scan-02.jpg", "scan-03.jpg"]);
        // сто с лишним файлов — номера с нулями до трёх знаков
        let many: Vec<(String, bool)> = (0..120).map(|i| (format!("f{i}.txt"), false)).collect();
        let r = Rule { num: Num::Instead, ..Rule::default() };
        let p = plan(&many, &r);
        assert_eq!((p[0].as_str(), p[119].as_str()), ("001.txt", "120.txt"));
    }

    #[test]
    fn a_folder_has_no_extension_to_keep() {
        let r = Rule { find: ".d".into(), replace: "-d".into(), ..Rule::default() };
        assert_eq!(rename_one("my.d", true, 0, 1, &r), "my-d");
    }

    #[test]
    fn problems_are_found_before_anything_is_renamed() {
        let old = ["a.txt", "b.txt", "c.txt"].map(String::from).to_vec();
        let exists = |n: &str| n == "taken.txt" || old.iter().any(|o| o == n);
        let new = ["same.txt", "same.txt", "c.txt"].map(String::from).to_vec();
        let p = check(&old, &new, &exists);
        assert_eq!((p[0], p[1], p[2]), (Some(Problem::Duplicate), Some(Problem::Duplicate), None), "c.txt is unchanged");
        let p = check(&old, &["taken.txt", "ok.txt", "c.txt"].map(String::from).to_vec(), &exists);
        assert_eq!(p[0], Some(Problem::Exists));
        assert_eq!(p[1], None);
        // имя другого выделенного файла не мешает: он сам уйдёт (обмен)
        let p = check(&old, &["b.txt", "a.txt", "c.txt"].map(String::from).to_vec(), &exists);
        assert_eq!((p[0], p[1]), (None, None));
        let p = check(&old, &["", "x/y", "c.txt"].map(String::from).to_vec(), &exists);
        assert_eq!((p[0], p[1]), (Some(Problem::Empty), Some(Problem::Slash)));
    }

    fn files(d: &Path, names: &[&str]) {
        for n in names {
            std::fs::write(d.join(n), *n).unwrap();
        }
    }

    #[test]
    fn two_files_can_swap_names_and_the_contents_follow() {
        let d = nemo_px::paths::private_tmp("batch-swap").unwrap();
        files(&d, &["a.txt", "b.txt"]);
        let old = ["a.txt", "b.txt"].map(String::from).to_vec();
        let new = ["b.txt", "a.txt"].map(String::from).to_vec();
        let pairs = apply(&d, &old, &new, &|a, b| std::fs::rename(a, b)).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "a.txt", "what was a is now b");
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "b.txt");
        assert_eq!(pairs.len(), 2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_failure_in_the_middle_puts_every_name_back() {
        let d = nemo_px::paths::private_tmp("batch-fail").unwrap();
        files(&d, &["a.txt", "b.txt", "c.txt"]);
        let old = ["a.txt", "b.txt", "c.txt"].map(String::from).to_vec();
        let new = ["x1.txt", "x2.txt", "x3.txt"].map(String::from).to_vec();
        let calls = std::cell::Cell::new(0);
        // шестая операция (третья из второго шага) падает
        let r = apply(&d, &old, &new, &|a, b| {
            calls.set(calls.get() + 1);
            if calls.get() == 6 {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                std::fs::rename(a, b)
            }
        });
        assert!(r.is_err());
        let mut names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names, ["a.txt", "b.txt", "c.txt"], "nothing lost, nothing left half-renamed");
        for n in ["a.txt", "b.txt", "c.txt"] {
            assert_eq!(std::fs::read_to_string(d.join(n)).unwrap(), n, "{n} still has its own content");
        }
        let _ = std::fs::remove_dir_all(d);
    }
}
