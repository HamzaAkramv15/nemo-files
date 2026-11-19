// Что ставит `nemo-files install`: файлы, вшитые в бинарь, и вопросы к человеку (nemo_px::install).

use nemo_px::install::{Asset, Choice, Spec};

pub const SPEC: Spec = Spec {
    name: "nemo-files",
    version: env!("CARGO_PKG_VERSION"),
    about: ["файловый менеджер и диалог выбора файлов", "file manager and file dialog", "gerenciador e diálogo de arquivos"], // i18n-ok
    deps: "wayland, libxkbcommon; optional: ffmpegthumbnailer, libarchive, gvfs, udisks2",
    assets: &[Asset { path: "applications/nemo-files.desktop", body: include_str!("../config/nemo-files.desktop") }],
    choices: &[
        Choice {
            ask: ["Открывать папки в nemo-files по умолчанию?", "Open folders in nemo-files by default?", "Abrir pastas no nemo-files por padrão?"], // i18n-ok
            assets: &[],
            default_app: true,
            note: None,
        },
        Choice {
            ask: ["Зарегистрировать диалог выбора файлов для всех программ (портал)?", "Register the file dialog for every app (portal)?", "Registrar o diálogo de arquivos para todos os apps (portal)?"], // i18n-ok
            assets: &[
                Asset { path: "xdg-desktop-portal/portals/nemo.portal", body: include_str!("../config/nemo.portal") },
                Asset { path: "dbus-1/services/org.freedesktop.impl.portal.desktop.nemo.service", body: include_str!("../config/org.freedesktop.impl.portal.desktop.nemo.service") },
            ],
            default_app: false,
            note: Some(["Осталось добавить в portals.conf: org.freedesktop.impl.portal.FileChooser=nemo;gtk;", "Now add to portals.conf: org.freedesktop.impl.portal.FileChooser=nemo;gtk;", "Agora adicione ao portals.conf: org.freedesktop.impl.portal.FileChooser=nemo;gtk;"]), // i18n-ok
        },
        Choice {
            ask: ["Показывать файлы в папке через nemo-files для всех программ?", "Use nemo-files for «show in folder» in every app?", "Usar o nemo-files para «mostrar na pasta» em todos os apps?"], // i18n-ok
            assets: &[Asset { path: "dbus-1/services/org.freedesktop.FileManager1.service", body: include_str!("../config/org.freedesktop.FileManager1.service") }],
            default_app: false,
            note: None,
        },
    ],
};
