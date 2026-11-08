// Что ставит `yami-files install`: файлы, вшитые в бинарь, и вопросы к человеку (yami_px::install).

use yami_px::install::{Asset, Choice, Spec};

pub const SPEC: Spec = Spec {
    name: "yami-files",
    version: env!("CARGO_PKG_VERSION"),
    about: ["файловый менеджер и диалог выбора файлов", "file manager and file dialog", "gerenciador e diálogo de arquivos"], // i18n-ok
    deps: "wayland, libxkbcommon; optional: ffmpegthumbnailer, libarchive, gvfs, udisks2",
    assets: &[Asset { path: "applications/yami-files.desktop", body: include_str!("../config/yami-files.desktop") }],
    choices: &[
        Choice {
            ask: ["Открывать папки в yami-files по умолчанию?", "Open folders in yami-files by default?", "Abrir pastas no yami-files por padrão?"], // i18n-ok
            assets: &[],
            default_app: true,
            note: None,
        },
        Choice {
            ask: ["Зарегистрировать диалог выбора файлов для всех программ (портал)?", "Register the file dialog for every app (portal)?", "Registrar o diálogo de arquivos para todos os apps (portal)?"], // i18n-ok
            assets: &[
                Asset { path: "xdg-desktop-portal/portals/yami.portal", body: include_str!("../config/yami.portal") },
                Asset { path: "dbus-1/services/org.freedesktop.impl.portal.desktop.yami.service", body: include_str!("../config/org.freedesktop.impl.portal.desktop.yami.service") },
            ],
            default_app: false,
            note: Some(["Осталось добавить в portals.conf: org.freedesktop.impl.portal.FileChooser=yami;gtk;", "Now add to portals.conf: org.freedesktop.impl.portal.FileChooser=yami;gtk;", "Agora adicione ao portals.conf: org.freedesktop.impl.portal.FileChooser=yami;gtk;"]), // i18n-ok
        },
        Choice {
            ask: ["Показывать файлы в папке через yami-files для всех программ?", "Use yami-files for «show in folder» in every app?", "Usar o yami-files para «mostrar na pasta» em todos os apps?"], // i18n-ok
            assets: &[Asset { path: "dbus-1/services/org.freedesktop.FileManager1.service", body: include_str!("../config/org.freedesktop.FileManager1.service") }],
            default_app: false,
            note: None,
        },
    ],
};
