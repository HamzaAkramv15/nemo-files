// Отрисовка окон-вопросов файлов (Dlg): рамка с проявлением, кнопки и содержимое каждого окна
// (новая папка, переименование, конфликты, «открыть с помощью», сервер, свойства, настройки).

use crate::draw::*;
use crate::act::{Act, Ui};
use crate::fm::{Dlg, Fm, Mode, NameKind};
use crate::font::PX;
use crate::fsx::{self, Sort};
use crate::gfx::{BoxStyle, C, R};
use crate::mime::{self};
use crate::picons::{self, Special};
use crate::ui::{Cur, Tone};
use std::time::Instant;

// ── окна-вопросы ────────────────────────────────────────────────────────────
pub(crate) fn dlg_frame(ui: &mut Ui, fm: &Fm, born: Instant, w: i32, h: i32, dw: i32, dh: i32, icon: &str, title: &str) -> R {
    let t = ui.t;
    let k = ease(born, 220.0, fm);
    ui.cv.fill(R::new(0, 0, w, h), t.bg.alpha(0.55 * k));
    ui.zone(R::new(0, 0, w, h), Act::Dim, Cur::Default);
    let rise = ((1.0 - k) * 12.0) as i32;
    let r = R::new((w - dw) / 2, (h - dh) / 2 + rise, dw, dh);
    ui.cv.pxbox(r, &BoxStyle::new(t.panel.alpha(0.99), t.border.lerp(t.red, 0.5)).shadow(6));
    ui.zone(r, Act::Block, Cur::Default);
    let hr = R::new(r.x + 1, r.y + 1, r.w - 2, 34);
    ui.cv.fill(hr, t.surface);
    ui.cv.fill(R::new(hr.x, hr.bottom(), hr.w, 1), t.border);
    ui.cv.icon(icon, hr.x + 12, hr.cy() - 7, 2, t.red);
    ui.text_in(PX, R::new(hr.x + 34, hr.y, hr.w - 80, hr.h), title, t.text);
    ui.button(R::new(hr.right() - 30, hr.y + 5, 24, 24), "", "close", Tone::Ghost, false, true, Act::Dlg("cancel"));
    if k < 1.0 {
        ui.anim = true;
    }
    r
}

pub(crate) fn dissolve_dlg(ui: &mut Ui, fm: &Fm, born: Instant, r: R) {
    let k = ease(born, 220.0, fm);
    if k < 1.0 {
        ui.cv.dissolve_rect(R::new(r.x, r.y, r.w + 6, r.h + 6), k, 2);
    }
}

pub(crate) fn buttons(ui: &mut Ui, r: R, list: &[(&str, &'static str, Tone)]) {
    let mut x = r.right() - 14;
    for (label, act, tone) in list.iter().rev() {
        let bw = ui.w(PX, label) + 32;
        x -= bw;
        ui.button(R::new(x, r.bottom() - 46, bw, 32), label, "", *tone, false, true, Act::Dlg(act));
        x -= 8;
    }
}

pub(crate) fn dialog(ui: &mut Ui, fm: &Fm, d: &Dlg, born: Instant, w: i32, h: i32) {
    let t = ui.t;
    match d {
        Dlg::Name { kind, f, err } => {
            let title = match kind {
                NameKind::NewFolder => "новая папка",
                NameKind::NewFile => "новый файл",
                NameKind::Compress => "сжать в архив",
                NameKind::Bookmark(_) => "имя в местах",
            };
            let r = dlg_frame(ui, fm, born, w, h, 440, 170, if matches!(kind, NameKind::Compress) { "copy" } else { "plus" }, title);
            let fr = R::new(r.x + 16, r.y + 52, r.w - 32, 34);
            ui.field(fr, f, "имя", true, false, Act::DlgField);
            if !err.is_empty() {
                ui.text_in(PX, R::new(r.x + 16, r.y + 92, r.w - 32, 20), err, t.warn);
            }
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), ("создать", "ok", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Server { addr, user, pass, field, err, question, busy } => {
            let dh = if question.is_some() { 330 } else { 280 };
            let r = dlg_frame(ui, fm, born, w, h, 480, dh, "net", "подключиться к серверу");
            let lx = r.x + 16;
            let fx = r.x + 110;
            let fw = r.right() - 16 - fx;
            let mut y = r.y + 50;
            let rows: [(&str, &str, &nemo_px::ui::Field, bool); 3] = [("адрес", "smb://nas/share", addr, false), ("логин", "необязательно", user, false), ("пароль", "", pass, true)];
            for (n, (label, ph, f, pw)) in rows.into_iter().enumerate() {
                ui.text_in(PX, R::new(lx, y, 90, 32), label, t.text_dim);
                let fr = R::new(fx, y, fw, 32);
                ui.field(fr, f, ph, *field as usize == n && !busy, pw, Act::DlgFocus(n as u8));
                y += 40;
            }
            ui.text_in(PX, R::new(lx, y, r.w - 32, 18), "smb · sftp · ftp · davs · nfs · домен — ДОМЕН\\логин", t.text_faint);
            y += 24;
            if let Some(q) = question {
                ui.cv.fill(R::new(lx, y + 6, 6, 6), t.warn);
                raw(ui, PX, R::new(lx + 14, y, r.w - 46, 18), q, t.warn);
                ui.text_in(PX, R::new(lx + 14, y + 20, r.w - 46, 18), "сервер просит подтверждения: подключиться всё равно?", t.text_dim);
            }
            if *busy {
                let dots = ".".repeat(1 + (born.elapsed().as_millis() / 300 % 3) as usize);
                raw(ui, PX, R::new(lx, r.bottom() - 40, r.w / 2, 20), &format!("{}{dots}", crate::i18n::tr("подключаю")), t.text_dim);
                ui.anim = true;
            } else if !err.is_empty() {
                raw(ui, PX, R::new(lx, r.bottom() - 70, r.w - 32, 20), err, t.warn);
            }
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), (if question.is_some() { "подключиться всё равно" } else { "подключить" }, "ok", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Storage { dir, items, done, free, .. } => {
            let r = dlg_frame(ui, fm, born, w, h, 600, 480, "gauge", "что занимает место");
            let lx = r.x + 16;
            let iw = r.w - 32;
            let total: u64 = items.iter().map(|i| i.1).sum();
            raw(ui, PX, R::new(lx, r.y + 46, iw, 18), &crate::fsx::pretty(dir), t.text);
            let sum = if *done { crate::tf!("всего {}", "total {}", "total {}", crate::fsx::human(total)) } else { crate::tf!("считаю… {}", "counting… {}", "contando… {}", crate::fsx::human(total)) };
            let free_s = free.map(|(f, _)| crate::tf!("  ·  свободно на диске {}", "  ·  {} free on the disk", "  ·  {} livres no disco", crate::fsx::human(f))).unwrap_or_default();
            raw(ui, PX, R::new(lx, r.y + 66, iw, 18), &format!("{sum}{free_s}"), t.text_dim);
            if !*done {
                ui.anim = true;
            }
            let max = items.first().map(|i| i.1).unwrap_or(0);
            let mut y = r.y + 96;
            let rows = ((r.bottom() - 60 - y) / 28).max(0) as usize;
            for (i, (name, size, is_dir)) in items.iter().take(rows).enumerate() {
                let row = R::new(lx, y, iw, 26);
                let hov = ui.hover(row);
                if hov {
                    ui.cv.fill(row, t.surface_hi.alpha(0.6));
                }
                ui.cv.icon(if *is_dir { "folder" } else { "copy" }, row.x + 4, row.y + 5, 1, t.text_faint);
                let nw = 200;
                let nm = ui.f.elide(PX, name, nw);
                raw(ui, PX, R::new(row.x + 24, row.y + 4, nw, 18), &nm, t.text);
                let bx = row.x + 24 + nw + 8;
                let bw = iw - 24 - nw - 8 - 86;
                ui.cv.fill(R::new(bx, row.y + 9, bw, 8), t.surface.lerp(t.border, 0.4));
                let fill = (bw as f32 * crate::storage::share(*size, max)).round() as i32;
                ui.cv.fill(R::new(bx, row.y + 9, fill.max(if *size > 0 { 2 } else { 0 }), 8), if *is_dir { t.red } else { t.ember });
                ui.cv.text_right(ui.f, PX, R::new(row.right() - 80, row.y + 4, 76, 18), &crate::fsx::human(*size), t.text_dim);
                ui.zone(row, Act::StorageGo(i), Cur::Pointer);
                y += 28;
            }
            if items.len() > rows && rows > 0 {
                raw(ui, PX, R::new(lx, y, iw, 18), &format!("… +{}", items.len() - rows), t.text_faint);
            }
            buttons(ui, r, &[("закрыть", "ok", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Batch { items, f, field, case, num, err } => {
            let r = dlg_frame(ui, fm, born, w, h, 580, 500, "copy", "переименовать несколько файлов");
            let lx = r.x + 16;
            let fx = r.x + 130;
            let fw = r.right() - 16 - fx;
            let mut y = r.y + 48;
            let rows: [(&str, &str); 4] = [("найти", "что заменить в имени"), ("заменить на", "чем (можно пусто)"), ("в начало", "приписка перед именем"), ("в конец", "приписка после имени")];
            for (n, (label, ph)) in rows.into_iter().enumerate() {
                ui.text_in(PX, R::new(lx, y, 110, 30), label, t.text_dim);
                ui.field(R::new(fx, y, fw, 30), &f[n], ph, *field as usize == n, false, Act::DlgFocus(n as u8));
                y += 36;
            }
            let half = (r.w - 32 - 10) / 2;
            let case_l = format!("{}: {}", crate::i18n::tr("регистр"), crate::i18n::tr(case.label()));
            let num_l = format!("{}: {}", crate::i18n::tr("номера"), crate::i18n::tr(num.label()));
            ui.button(R::new(lx, y, half, 28), &case_l, "", Tone::Plain, false, true, Act::BatchCase);
            ui.button(R::new(lx + half + 10, y, half, 28), &num_l, "", Tone::Plain, false, true, Act::BatchNum);
            y += 38;
            // что получится: пока человек печатает
            let rule = crate::batch::Rule { find: f[0].text.clone(), replace: f[1].text.clone(), prefix: f[2].text.clone(), suffix: f[3].text.clone(), case: *case, num: *num };
            let (old, new, probs) = crate::batch::preview(items, &rule);
            let changed = (0..old.len()).filter(|&i| old[i] != new[i]).count();
            let colw = (r.w - 32 - 24) / 2;
            for i in 0..old.len().min(7) {
                let bad = probs[i].is_some();
                let col = if bad { t.warn } else if old[i] == new[i] { t.text_faint } else { t.text };
                let a = ui.f.elide(PX, &old[i], colw);
                raw(ui, PX, R::new(lx, y, colw, 18), &a, t.text_dim);
                ui.cv.icon("right", lx + colw + 4, y + 2, 1, t.text_faint);
                let b = ui.f.elide(PX, &new[i], colw);
                raw(ui, PX, R::new(lx + colw + 24, y, colw, 18), &b, col);
                y += 19;
            }
            if old.len() > 7 {
                raw(ui, PX, R::new(lx, y, r.w - 32, 18), &format!("… +{}", old.len() - 7), t.text_faint);
            }
            let status = if !err.is_empty() { (err.clone(), t.warn) } else { (crate::tf!("изменится: {} из {} · F3 регистр · F4 номера · Tab поля", "will change: {} of {} · F3 case · F4 numbers · Tab fields", "vai mudar: {} de {} · F3 maiúsculas · F4 números · Tab campos", changed, old.len()), t.text_faint) };
            raw(ui, PX, R::new(lx, r.bottom() - 74, r.w - 32, 20), &status.0, status.1);
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), ("переименовать", "ok", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Unlock { name, pass, err, busy, .. } => {
            let r = dlg_frame(ui, fm, born, w, h, 460, 210, "lock", "зашифрованный диск");
            raw(ui, PX, R::new(r.x + 16, r.y + 48, r.w - 32, 20), name, t.text);
            ui.field(R::new(r.x + 16, r.y + 76, r.w - 32, 32), pass, "пароль диска", !busy, true, Act::DlgField);
            if *busy {
                let dots = ".".repeat(1 + (born.elapsed().as_millis() / 300 % 3) as usize);
                raw(ui, PX, R::new(r.x + 16, r.y + 116, r.w - 32, 20), &format!("{}{dots}", crate::i18n::tr("открываю")), t.text_dim);
                ui.anim = true;
            } else if !err.is_empty() {
                raw(ui, PX, R::new(r.x + 16, r.y + 116, r.w - 32, 20), err, t.warn);
            } else {
                ui.text_in(PX, R::new(r.x + 16, r.y + 116, r.w - 32, 20), "пароль не сохраняется", t.text_faint);
            }
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), ("открыть", "ok", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Confirm { title, text, ok, .. } => {
            let r = dlg_frame(ui, fm, born, w, h, 460, 160, "trash", title);
            let lines = label_lines(ui.f, PX, crate::i18n::tr(text), r.w - 32, 3);
            for (i, l) in lines.iter().enumerate() {
                raw(ui, PX, R::new(r.x + 16, r.y + 50 + i as i32 * 20, r.w - 32, 20), l, t.text);
            }
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), (ok.as_str(), "ok", Tone::Danger)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Conflict { src, dst, all, .. } => {
            let r = dlg_frame(ui, fm, born, w, h, 540, 250, "copy", "такой файл уже есть");
            let name = dst.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            raw(ui, PX, R::new(r.x + 16, r.y + 48, r.w - 32, 20), &name, t.text);
            let (a, b) = (fsx::Entry::from_path(src), fsx::Entry::from_path(dst));
            let mut y = r.y + 76;
            for (label, e) in [("новый", a), ("уже есть", b)] {
                ui.text_in(PX, R::new(r.x + 16, y, 100, 20), label, t.text_faint);
                if let Some(e) = e {
                    let s = format!("{} · {}", if e.dir { crate::i18n::tr("папка").to_string() } else { fsx::human(e.size) }, fsx::when(e.mtime));
                    raw(ui, PX, R::new(r.x + 120, y, r.w - 136, 20), &s, t.text_dim);
                }
                y += 22;
            }
            ui.toggle(R::new(r.x + 16, r.y + 130, 300, 24), "для всех остальных", "", *all, Act::Dlg("all"));
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), ("пропустить", "skip", Tone::Plain), ("оставить оба", "rename", Tone::Plain), ("заменить", "replace", Tone::Danger)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Errors(errs) => {
            let r = dlg_frame(ui, fm, born, w, h, 520, 120 + 20 * errs.len().min(8) as i32, "close", "не всё получилось");
            for (i, e) in errs.iter().take(8).enumerate() {
                raw(ui, PX, R::new(r.x + 16, r.y + 48 + i as i32 * 20, r.w - 32, 20), e, t.text_dim);
            }
            if errs.len() > 8 {
                ui.text_in(PX, R::new(r.x + 16, r.y + 48 + 8 * 20, r.w - 32, 20), &crate::tf!("и ещё {}", "and {} more", "e mais {}", errs.len() - 8), t.text_faint);
            }
            buttons(ui, r, &[("понятно", "cancel", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Props { paths, du, free, .. } => props(ui, fm, born, w, h, paths, du, *free),
        Dlg::OpenWith { mime: m, f, sel, remember, scroll, files } => {
            let r = dlg_frame(ui, fm, born, w, h, 480, (h - 80).min(520), "dots", "открыть с помощью");
            let what = if files.len() == 1 { files[0].file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() } else { crate::tf!("файлов: {}", "{} files", "{} arquivos", files.len()) };
            raw(ui, PX, R::new(r.x + 16, r.y + 44, r.w - 32, 20), &format!("{what} · {}", mime::label(m, "")), t.text_dim);
            ui.field(R::new(r.x + 16, r.y + 70, r.w - 32, 32), f, "найти программу", true, false, Act::DlgField);
            let lr = R::new(r.x + 12, r.y + 110, r.w - 24, r.h - 110 - 92);
            ui.cv.push_clip(lr);
            let list = crate::dialogs::open_list_cache(fm);
            let rh = 30;
            let mut y = lr.y - *scroll;
            let mut sep_drawn = false;
            for (n, (idx, fits)) in list.iter().enumerate() {
                if !fits && !sep_drawn && n > 0 {
                    ui.text_in(PX, R::new(lr.x + 6, y + 6, lr.w, 18), "остальные программы", t.text_faint);
                    y += 26;
                    sep_drawn = true;
                }
                if y + rh >= lr.y && y <= lr.bottom() {
                    if let Some(a) = fm.apps_ref().and_then(|ap| ap.all.get(*idx)) {
                        let row = R::new(lr.x, y, lr.w, rh - 2);
                        let hov = ui.hover(row);
                        if n == *sel {
                            ui.cv.pxbox(row, &BoxStyle::new(t.blood, C(0)));
                        } else if hov {
                            ui.cv.pxbox(row, &BoxStyle::new(t.surface_hi, C(0)));
                        }
                        ui.app_icon(&a.icon, row.x + 6, row.y + 3, 22, 1.0);
                        raw(ui, PX, R::new(row.x + 36, row.y, row.w / 2, row.h), &a.name, if n == *sel { t.text_hi } else { t.text });
                        let gw = ui.f.width(PX, &a.name);
                        raw(ui, PX, R::new(row.x + 46 + gw, row.y, row.w - gw - 56, row.h), &a.generic, if n == *sel { t.text_dim.lerp(t.text_hi, 0.4) } else { t.text_faint });
                        ui.zone(row.inter(&lr), Act::DlgApp(n), Cur::Pointer);
                    }
                }
                y += rh;
            }
            ui.cv.pop_clip();
            ui.toggle(R::new(r.x + 16, r.bottom() - 82, 320, 24), "всегда открывать этот тип так", "", *remember, Act::Dlg("remember"));
            buttons(ui, r, &[("отмена", "cancel", Tone::Plain), ("открыть", "ok", Tone::Primary)]);
            dissolve_dlg(ui, fm, born, r);
        }
        Dlg::Settings => settings(ui, fm, born, w, h),
    }
}

/// список программ для «открыть с помощью» считает Fm (нужна изменяемость); кадр берёт готовый
pub fn open_list_cache(fm: &Fm) -> Vec<(usize, bool)> {
    fm.open_list.borrow().clone()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn props(ui: &mut Ui, fm: &Fm, born: Instant, w: i32, h: i32, paths: &[std::path::PathBuf], du: &Option<((u64, u64, u64), bool)>, free: Option<(u64, u64)>) {
    let t = ui.t;
    let r = dlg_frame(ui, fm, born, w, h, 480, 400, "about", "свойства");
    let one = paths.len() == 1;
    let e = if one { fsx::Entry::from_path(&paths[0]) } else { None };
    let mut y = r.y + 50;
    let x = r.x + 18;
    if let Some(e) = &e {
        let sp = if e.dir { crate::places::special_of(&e.path) } else { Special::None };
        picons::draw(&mut ui.cv, &t, mime::kind(e.mime), sp, x, y, 3, picons::Look { dim: false, link: e.link, selected: false });
        raw(ui, PX, R::new(x + 62, y + 4, r.w - 100, 20), &e.name, t.text);
        ui.cv.text_in(ui.f, PX, R::new(x + 62, y + 26, r.w - 100, 18), &mime::label(e.mime, &e.name), t.text_dim);
        y += 64;
    } else {
        ui.text_in(PX, R::new(x, y, r.w - 36, 20), &crate::tf!("выбрано объектов: {}", "{} items selected", "{} itens selecionados", paths.len()), t.text);
        y += 30;
    }
    ui.cv.fill(R::new(x, y, r.w - 36, 1), t.border);
    y += 10;
    let mut kv = |ui: &mut Ui, k: &str, v: &str| {
        ui.text_in(PX, R::new(x, y, 110, 20), k, t.text_faint);
        raw(ui, PX, R::new(x + 112, y, r.w - 150, 20), v, t.text_dim);
        y += 22;
    };
    let size = match du {
        Some(((b, f, d), done)) => {
            let s = if *d > 0 { crate::tf!("{} · файлов {} · папок {}", "{} · {} files · {} folders", "{} · {} arquivos · {} pastas", fsx::human(*b), f, d) } else { fsx::human(*b) };
            if *done {
                s
            } else {
                s + " …"
            }
        }
        None => crate::i18n::tr("считаю…").to_string(),
    };
    if du.map(|d| !d.1).unwrap_or(true) {
        ui.anim = true;
    }
    kv(ui, "размер", &size);
    if let Some(e) = &e {
        kv(ui, "где", &e.path.parent().map(fsx::pretty).unwrap_or_default());
        kv(ui, "изменён", &fsx::full_time(e.mtime));
        kv(ui, "владелец", &format!("{} : {}", fsx::id_name("/etc/passwd", e.uid), fsx::id_name("/etc/group", e.gid)));
        if e.link {
            if let Ok(tg) = std::fs::read_link(&e.path) {
                kv(ui, "ссылка на", &tg.display().to_string());
            }
        }
        if let Some((free, total)) = free {
            kv(ui, "на диске", &crate::tf!("свободно {} из {}", "{} free of {}", "{} livres de {}", fsx::human(free), fsx::human(total)));
        }
        // права: три строки по три флажка
        y += 6;
        ui.text_in(PX, R::new(x, y, 110, 20), "права", t.text_faint);
        let cols = ["чтение", "запись", "запуск"];
        for (ci, c) in cols.iter().enumerate() {
            ui.text_in(PX, R::new(x + 112 + ci as i32 * 92, y, 90, 20), c, t.text_faint);
        }
        y += 22;
        for (ri, who) in ["владелец", "группа", "остальные"].iter().enumerate() {
            ui.text_in(PX, R::new(x, y, 110, 22), who, t.text_dim);
            for ci in 0..3 {
                let bit = 1u32 << ((2 - ri) * 3 + (2 - ci));
                let on = e.mode & bit != 0;
                let b = R::new(x + 112 + ci as i32 * 92, y + 3, 16, 16);
                let hov = ui.hover(b.inset(-3));
                ui.cv.pxbox(b, &BoxStyle::new(t.code_bg, if hov { t.red } else { t.border }));
                if on {
                    ui.cv.fill(R::new(b.x + 4, b.y + 4, 8, 8), t.ember);
                }
                ui.zone(R::new(b.x - 3, b.y - 3, 22, 22), Act::DlgBit(bit), Cur::Pointer);
            }
            y += 24;
        }
    }
    buttons(ui, r, &[("закрыть", "cancel", Tone::Primary)]);
    dissolve_dlg(ui, fm, born, r);
}

pub(crate) fn settings(ui: &mut Ui, fm: &Fm, born: Instant, w: i32, h: i32) {
    let t = ui.t;
    let r = dlg_frame(ui, fm, born, w, h, 560, 520, "gear", "настройки файлов");
    let x = r.x + 18;
    let cw = (r.w - 52) / 2;
    let mut y = r.y + 50;
    let c = &fm.cfg;
    let col2 = x + cw + 16;
    heading(ui, x, y, cw, "вид");
    heading(ui, col2, y, cw, "поведение");
    y += 24;
    let rows_l: [(&str, &str, bool, &str); 5] = [
        ("скрытые файлы", "ctrl+h", c.hidden, "hidden"),
        ("папки сверху", "", c.folders_first, "foldersFirst"),
        ("миниатюры", "картинки и видео", c.thumbs, "thumbs"),
        ("места слева", "", c.sidebar, "sidebar"),
        ("сведения справа", "f11", c.info, "info"),
    ];
    let rows_r: [(&str, &str, bool, &str); 6] = [
        ("открывать одним щелчком", "", c.single_click, "singleClick"),
        ("спрашивать перед корзиной", "", c.confirm_trash, "confirmTrash"),
        ("новое окно — две панели", "f3", c.split, "split"),
        ("по убыванию", "", c.desc, "desc"),
        ("устройства слева", "диски, телефоны", c.devices, "devices"),
        ("сеть слева", "smb, sftp, ftp", c.network, "network"),
    ];
    let mut yl = y;
    for (label, hint, on, key) in rows_l {
        ui.toggle(R::new(x, yl, cw, 40), label, hint, on, Act::SBool(key));
        yl += 44;
    }
    let mut yr = y;
    for (label, hint, on, key) in rows_r {
        ui.toggle(R::new(col2, yr, cw, 40), label, hint, on, Act::SBool(key));
        yr += 44;
    }
    // вид и сортировка — переключатели по кругу
    let cyc = |ui: &mut Ui, r2: R, label: &str, val: &str, key: &'static str| {
        ui.text_in(PX, R::new(r2.x, r2.y, 120, r2.h), label, t.text);
        let br = R::new(r2.x + 130, r2.y, r2.w - 130, r2.h);
        ui.button(br, val, "", Tone::Plain, false, true, Act::SCycle(key));
    };
    let mode = if fm.mode() == Mode::List { "список" } else { "значки" };
    let sort = match Sort::from_id(&c.sort) {
        Sort::Name => "по имени",
        Sort::Size => "по размеру",
        Sort::Time => "по дате",
        Sort::Kind => "по типу",
    };
    yr += 4;
    cyc(ui, R::new(col2, yr, cw, 28), "вид", mode, "view");
    yr += 34;
    cyc(ui, R::new(col2, yr, cw, 28), "порядок", sort, "sort");
    let yb = yl.max(y) + 6;
    ui.slider(R::new(x, yb, cw, 40), "размер значков", (c.zoom + 1) as f32 / 5.0, &format!("{}px", 16 * c.icon_u()), 5, Act::SZoom);
    ui.text_in(PX, R::new(x, r.bottom() - 70, r.w - 36, 18), "вид и язык: ~/.config/nemo/style.json", t.text_faint);
    raw(ui, PX, R::new(x, r.bottom() - 50, r.w / 2, 18), &format!("{}: {}", crate::i18n::tr("терминал"), c.terminal), t.text_faint);
    buttons(ui, r, &[("готово", "cancel", Tone::Primary)]);
    dissolve_dlg(ui, fm, born, r);
}
