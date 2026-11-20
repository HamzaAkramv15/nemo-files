// Окна-вопросы файлового менеджера (Dlg): новая папка, переименование, конфликты, свойства,
// «открыть с помощью», подключение к серверу; их ответы превращаются в команды.

use crate::fm::*;
use crate::fsx::{self};
use crate::ops;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

impl Fm {
    // ── окна-вопросы ───────────────────────────────────────────────────────
    pub fn dlg_ok(&mut self) {
        if matches!(self.dlg, Some((Dlg::Server { .. }, _))) {
            return self.server_ok();
        }
        if let Some((Dlg::Unlock { dev, pass, busy, err, .. }, _)) = &mut self.dlg {
            if *busy || pass.text.is_empty() {
                return;
            }
            *busy = true;
            err.clear();
            let mut auth = crate::remote::Auth::default();
            auth.pass = pass.text.clone();
            let dev = dev.clone();
            pass.wipe();
            return self.remote_mount(dev, auth);
        }
        let dir = self.v().dir.clone();
        let Some((d, born)) = self.dlg.take() else { return };
        match d {
            Dlg::Name { kind: NameKind::Bookmark(p), f, .. } => {
                crate::places::rename_bookmark(&p, &f.text);
                self.reload_places();
            }
            Dlg::Name { kind, f, .. } => {
                let name = f.text.trim().to_string();
                let err = if name.is_empty() {
                    Some("имя пустое")
                } else if name.contains('/') {
                    Some("в имени не может быть «/»")
                } else if std::fs::symlink_metadata(dir.join(&name)).is_ok() {
                    Some("такое имя уже есть")
                } else {
                    None
                };
                if let Some(e) = err {
                    self.dlg = Some((Dlg::Name { kind, f, err: e.into() }, born));
                    return;
                }
                let p = dir.join(&name);
                let r = match kind {
                    NameKind::NewFolder => std::fs::create_dir(&p),
                    NameKind::NewFile => std::fs::OpenOptions::new().write(true).create_new(true).open(&p).map(|_| ()),
                    NameKind::Compress => {
                        let t = self.v().targets();
                        self.job(ops::Kind::Compress, t, dir.clone(), name);
                        return;
                    }
                    NameKind::Bookmark(_) => return,
                };
                match r {
                    Ok(()) => {
                        self.undo.push(Undo::Created(vec![p.clone()]));
                        self.vm().pending_sel = vec![p];
                        self.reload_all_of(&dir);
                    }
                    Err(e) => self.dlg = Some((Dlg::Name { kind, f, err: fsx::err_text(&e) }, born)),
                }
            }
            Dlg::Batch { items, f, field, case, num, err: _ } => {
                let rule = crate::batch::Rule { find: f[0].text.clone(), replace: f[1].text.clone(), prefix: f[2].text.clone(), suffix: f[3].text.clone(), case, num };
                let (old, new, probs) = crate::batch::preview(&items, &rule);
                let again = |fm: &mut Fm, err: String, items, f| fm.dlg = Some((Dlg::Batch { items, f, field, case, num, err }, born));
                // файлы из разных папок (результаты поиска) — не переименовываем разом
                let parent = items.first().and_then(|(p, _)| p.parent()).map(PathBuf::from).unwrap_or_default();
                if items.iter().any(|(p, _)| p.parent() != Some(parent.as_path())) {
                    return again(self, crate::i18n::tr("файлы из разных папок").to_string(), items, f);
                }
                if let Some(bad) = probs.iter().filter_map(|p| *p).next() {
                    let n = probs.iter().filter(|p| p.is_some()).count();
                    return again(self, crate::tf!("{}: {} (всего проблем: {})", "{}: {} (problems: {})", "{}: {} (problemas: {})", crate::i18n::tr("нельзя"), crate::i18n::tr(bad.text()), n), items, f);
                }
                if old == new {
                    return again(self, crate::i18n::tr("ничего не изменится").to_string(), items, f);
                }
                match crate::batch::apply(&parent, &old, &new, &|a, b| std::fs::rename(a, b)) {
                    Ok(pairs) => {
                        let n = pairs.len();
                        self.vm().pending_sel = pairs.iter().map(|(_, b)| b.clone()).collect();
                        self.undo.push(Undo::Moved(pairs));
                        self.reload_all_of(&parent);
                        self.toast(&crate::tf!("переименовано: {}", "renamed: {}", "renomeados: {}", n));
                    }
                    Err(e) => again(self, e, items, f),
                }
            }
            Dlg::Confirm { cmd, .. } => self.run(cmd),
            Dlg::OpenWith { .. } => {
                self.dlg = Some((d, born));
                let list = self.open_with_list();
                let sel = match &self.dlg {
                    Some((Dlg::OpenWith { sel, .. }, _)) => *sel,
                    _ => 0,
                };
                if let Some((i, _)) = list.get(sel) {
                    self.open_with_run(*i);
                }
            }
            Dlg::Conflict { reply, all, .. } => {
                let _ = reply.send((ops::Resolve::Rename, all));
            }
            Dlg::Props { stop, .. } | Dlg::Storage { stop, .. } => stop.store(true, std::sync::atomic::Ordering::Relaxed),
            Dlg::Settings | Dlg::Errors(_) | Dlg::Server { .. } | Dlg::Unlock { .. } => {}
        }
    }

    pub fn dlg_cancel(&mut self) {
        match self.dlg.take() {
            Some((Dlg::Conflict { reply, .. }, _)) => {
                let _ = reply.send((ops::Resolve::Cancel, false));
            }
            Some((Dlg::Props { stop, .. }, _)) | Some((Dlg::Storage { stop, .. }, _)) => stop.store(true, std::sync::atomic::Ordering::Relaxed),
            Some((Dlg::Server { mut pass, .. }, _)) | Some((Dlg::Unlock { mut pass, .. }, _)) => pass.wipe(),
            _ => {}
        }
    }

    pub fn open_props(&mut self, paths: Vec<PathBuf>) {
        self.du_gen += 1;
        let gen = self.du_gen;
        let stop = Arc::new(AtomicBool::new(false));
        let has_dir = paths.iter().any(|p| p.is_dir() && !p.is_symlink());
        if has_dir {
            if let Some(tx) = &self.tx {
                let tx = tx.clone();
                let ps = paths.clone();
                let st = stop.clone();
                std::thread::spawn(move || {
                    let mut tot = (0u64, 0u64, 0u64);
                    for p in &ps {
                        if p.is_dir() {
                            let (b, f, d) = fsx::du(p, &st);
                            tot = (tot.0 + b, tot.1 + f, tot.2 + d + 1);
                        } else {
                            tot.0 += std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                            tot.1 += 1;
                        }
                        let _ = tx.send(Msg::Du { gen, v: tot, done: false });
                    }
                    let _ = tx.send(Msg::Du { gen, v: tot, done: true });
                });
            }
        }
        let du = if has_dir { None } else { Some(((paths.iter().map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)).sum(), paths.len() as u64, 0), true)) };
        // место на диске: папка панели уже замерена в нити чтения; иначе — один раз сейчас
        let v = self.v();
        let free = match paths.first() {
            Some(p) if p.parent() == Some(v.dir.as_path()) && v.free.is_some() => v.free,
            Some(p) => fsx::disk_free(p),
            None => None,
        };
        self.dlg = Some((Dlg::Props { paths, gen, du, stop, free }, Instant::now()));
    }

    /// окно «что занимает место» для папки панели: счёт в нити, порции приходят сообщениями
    pub fn open_storage(&mut self) {
        let dir = self.v().dir.clone();
        if self.v().trash || self.v().search.is_some() {
            return;
        }
        self.du_gen += 1;
        let gen = self.du_gen;
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(tx) = &self.tx {
            let (tx, d, st) = (tx.clone(), dir.clone(), stop.clone());
            std::thread::spawn(move || {
                crate::storage::scan(&d, &st, |items, done| {
                    let _ = tx.send(Msg::Storage { gen, items: items.to_vec(), done });
                });
            });
        }
        let free = fsx::disk_free(&dir);
        self.dlg = Some((Dlg::Storage { dir, gen, items: vec![], done: false, stop, free }, Instant::now()));
    }

    pub fn on_storage(&mut self, g: u32, new: Vec<crate::storage::Item>, fin: bool) {
        if let Some((Dlg::Storage { gen, items, done, .. }, _)) = &mut self.dlg {
            if *gen == g {
                *items = new;
                *done = fin;
            }
        }
    }

    pub fn on_du(&mut self, g: u32, v: (u64, u64, u64), done: bool) {
        if let Some((Dlg::Props { gen, du, .. }, _)) = &mut self.dlg {
            if *gen == g {
                *du = Some((v, done));
            }
        }
    }

    pub fn chmod(&mut self, bit: u32) {
        let Some((Dlg::Props { paths, .. }, _)) = &self.dlg else { return };
        for p in paths.clone() {
            if let Ok(m) = std::fs::metadata(&p) {
                use std::os::unix::fs::PermissionsExt;
                let mode = m.permissions().mode() ^ bit;
                if let Err(e) = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)) {
                    self.toast(fsx::err_text(&e));
                }
            }
        }
        let d = self.v().dir.clone();
        self.reload_all_of(&d);
    }
}
