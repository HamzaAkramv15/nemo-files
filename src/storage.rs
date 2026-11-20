// «Что занимает место»: размер каждого элемента текущей папки, самые тяжёлые сверху. Считает отдельная нить, пока окно
// открыто (большая папка — это секунды), и присылает список порциями, так что полосы растут на глазах; закрыли окно — счёт
// останавливается. Ссылки не раскрываются (ни петель, ни чужих дисков).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// имя, размер в байтах, папка ли
pub type Item = (String, u64, bool);

/// размер каждого дочернего элемента `dir`; `report` зовётся не чаще чем раз в 120 мс и в конце (done = true), список уже
/// по убыванию размера. Остановлено — выходит, не досчитав, и больше не отчитывается
pub fn scan(dir: &Path, stop: &AtomicBool, mut report: impl FnMut(&[Item], bool)) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        report(&[], true);
        return;
    };
    let mut items: Vec<Item> = vec![];
    let mut last = Instant::now();
    for e in rd.flatten() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(m) = e.metadata() else { continue };
        let (size, is_dir) = if m.is_dir() { (crate::fsx::du(&e.path(), stop).0, true) } else if m.file_type().is_symlink() { (0, false) } else { (m.len(), false) };
        items.push((name, size, is_dir));
        if last.elapsed() >= Duration::from_millis(120) {
            last = Instant::now();
            sort(&mut items);
            report(&items, false);
        }
    }
    if stop.load(Ordering::Relaxed) {
        return;
    }
    sort(&mut items);
    report(&items, true);
}

pub fn sort(items: &mut [Item]) {
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
}

/// доля элемента от самого большого, 0..1 (для длины полосы)
pub fn share(size: u64, max: u64) -> f32 {
    if max == 0 {
        0.0
    } else {
        (size as f64 / max as f64).clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heaviest_comes_first_and_folders_are_counted_whole() {
        let d = nemo_px::paths::private_tmp("storage-test").unwrap();
        std::fs::write(d.join("small.txt"), vec![0u8; 10]).unwrap();
        std::fs::write(d.join("big.bin"), vec![0u8; 5000]).unwrap();
        std::fs::create_dir_all(d.join("folder/inner")).unwrap();
        std::fs::write(d.join("folder/a"), vec![0u8; 2000]).unwrap();
        std::fs::write(d.join("folder/inner/b"), vec![0u8; 4000]).unwrap();
        std::os::unix::fs::symlink(d.join("big.bin"), d.join("link")).unwrap();
        let stop = AtomicBool::new(false);
        let mut got: Vec<Item> = vec![];
        let mut finished = false;
        scan(&d, &stop, |i, done| {
            got = i.to_vec();
            finished = done;
        });
        assert!(finished);
        let names: Vec<&str> = got.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["folder", "big.bin", "small.txt", "link"], "6000, 5000, 10, and a link counts nothing");
        assert_eq!(got[0], ("folder".to_string(), 6000, true));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_stopped_scan_does_not_report_a_finished_list() {
        let d = nemo_px::paths::private_tmp("storage-stop").unwrap();
        std::fs::write(d.join("a"), "x").unwrap();
        let stop = AtomicBool::new(true);
        let mut called = false;
        scan(&d, &stop, |_, _| called = true);
        assert!(!called);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_unreadable_folder_reports_an_empty_finished_list() {
        let mut got = None;
        scan(Path::new("/nonexistent-dir-xyz"), &AtomicBool::new(false), |i, done| got = Some((i.len(), done)));
        assert_eq!(got, Some((0, true)));
    }

    #[test]
    fn shares_are_fractions_of_the_biggest() {
        assert_eq!(share(50, 100), 0.5);
        assert_eq!(share(5, 0), 0.0);
        assert_eq!(share(200, 100), 1.0);
    }
}
