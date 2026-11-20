// Фильтр по виду файла в строке фильтра: рядом с частью имени слова вида «:картинки», «:video», «:код». Несколько видов —
// «или» (`:картинки :видео` — и то и другое), остальные слова — часть имени. Вид определяется по mime и расширению:
// того, что файловая система и без того знает, ничего не читается из файла.

use crate::fsx::Entry;

/// виды: ключ, слова, которыми его называют (по-английски и по-русски)
const KINDS: &[(&str, &[&str])] = &[
    ("image", &[":image", ":images", ":img", ":картинки", ":картинка", ":фото"]),
    ("video", &[":video", ":videos", ":видео"]),
    ("audio", &[":audio", ":music", ":музыка", ":аудио"]),
    ("doc", &[":doc", ":docs", ":document", ":documents", ":документы", ":документ"]),
    ("archive", &[":archive", ":archives", ":zip", ":архивы", ":архив"]),
    ("code", &[":code", ":src", ":код"]),
    ("folder", &[":folder", ":folders", ":dir", ":папки", ":папка"]),
];

/// «имя :видео :картинки» → (виды, оставшийся текст); слово с двоеточием, не называющее вид, остаётся в тексте
pub fn parse_query(q: &str) -> (Vec<&'static str>, String) {
    let mut kinds: Vec<&'static str> = vec![];
    let mut text: Vec<&str> = vec![];
    for w in q.split_whitespace() {
        let lw = w.to_lowercase();
        match KINDS.iter().find(|(_, names)| names.contains(&lw.as_str())) {
            Some((k, _)) => {
                if !kinds.contains(k) {
                    kinds.push(k);
                }
            }
            None => text.push(w),
        }
    }
    (kinds, text.join(" "))
}

fn ext(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

pub fn is_kind(kind: &str, e: &Entry) -> bool {
    let x = ext(&e.name);
    let x = x.as_str();
    match kind {
        "folder" => e.dir,
        _ if e.dir => false,
        "image" => e.mime.starts_with("image/") || matches!(x, "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "tif" | "tiff" | "heic" | "ico"),
        "video" => e.mime.starts_with("video/") || matches!(x, "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "wmv" | "m4v" | "mpg" | "mpeg"),
        "audio" => e.mime.starts_with("audio/") || matches!(x, "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma"),
        "archive" => matches!(x, "zip" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "iso" | "deb" | "rpm" | "cab") || e.mime.contains("zip") || e.mime.contains("x-tar") || e.mime.contains("compressed"),
        "code" => matches!(x, "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "c" | "h" | "cpp" | "hpp" | "cc" | "go" | "java" | "kt" | "sh" | "bash" | "fish" | "lua" | "rb" | "php" | "toml" | "json" | "yaml" | "yml" | "css" | "html" | "htm" | "xml" | "sql" | "nix" | "kdl" | "mk" | "cmake" | "zig" | "swift"),
        "doc" => {
            matches!(x, "pdf" | "doc" | "docx" | "odt" | "rtf" | "epub" | "txt" | "md" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "odp" | "csv" | "tex" | "djvu")
                || e.mime == "application/pdf"
                || e.mime.starts_with("application/vnd.oasis")
                || e.mime.starts_with("application/vnd.openxmlformats")
                || e.mime == "text/plain"
        }
        _ => false,
    }
}

/// подходит ли запись: любой из видов (или виды не заданы) и часть имени
pub fn matches(e: &Entry, kinds: &[&str], text_lower: &str) -> bool {
    (kinds.is_empty() || kinds.iter().any(|k| is_kind(k, e))) && (text_lower.is_empty() || e.name.to_lowercase().contains(text_lower))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool, mime: &'static str) -> Entry {
        Entry { name: name.into(), path: name.into(), dir, link: false, broken: false, size: 0, mtime: 0, mode: 0, uid: 0, gid: 0, hidden: false, mime, orig: None, deleted: 0, place: None }
    }

    #[test]
    fn kinds_are_taken_from_the_query_and_the_rest_stays_a_name() {
        assert_eq!(parse_query("report :docs"), (vec!["doc"], "report".to_string()));
        assert_eq!(parse_query(":картинки :видео sunset"), (vec!["image", "video"], "sunset".to_string()));
        assert_eq!(parse_query(":img :IMAGES"), (vec!["image"], String::new()), "the same kind twice, any case");
        assert_eq!(parse_query("a:b :unknown"), (vec![], "a:b :unknown".to_string()), "an unknown word with a colon is part of the name");
        assert_eq!(parse_query(""), (vec![], String::new()));
    }

    #[test]
    fn files_fall_into_their_kinds_by_mime_or_extension() {
        assert!(is_kind("image", &e("a.PNG", false, "")));
        assert!(is_kind("image", &e("x", false, "image/webp")));
        assert!(is_kind("video", &e("a.mkv", false, "")));
        assert!(is_kind("audio", &e("a.flac", false, "")));
        assert!(is_kind("archive", &e("a.tar.gz", false, "")));
        assert!(is_kind("code", &e("main.rs", false, "")));
        assert!(is_kind("doc", &e("a.pdf", false, "")));
        assert!(is_kind("doc", &e("notes", false, "text/plain")));
        assert!(!is_kind("image", &e("a.txt", false, "")));
        assert!(is_kind("folder", &e("d", true, "")));
        assert!(!is_kind("image", &e("photos.png", true, "")), "a folder named like a picture is a folder");
    }

    #[test]
    fn several_kinds_mean_or_and_the_name_narrows_them() {
        let items = [e("a.png", false, ""), e("b.mkv", false, ""), e("c.txt", false, ""), e("sunset.png", false, "")];
        let hit = |k: &[&str], t: &str| items.iter().filter(|x| matches(x, k, t)).map(|x| x.name.clone()).collect::<Vec<_>>();
        assert_eq!(hit(&["image", "video"], ""), ["a.png", "b.mkv", "sunset.png"]);
        assert_eq!(hit(&["image"], "sun"), ["sunset.png"]);
        assert_eq!(hit(&[], "c."), ["c.txt"]);
        assert_eq!(hit(&[], ""), ["a.png", "b.mkv", "c.txt", "sunset.png"]);
    }
}
