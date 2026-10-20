//! Paint.NET's menu bar (File, Edit, View, Image, Layers, Adjustments, Effects), laid out over
//! PhotoCraft's commands. Every row names an existing command id; the labels are Paint.NET's, in
//! Russian when the UI language is Russian and in Paint.NET's English otherwise. A row whose
//! command PhotoCraft lacks is simply shown disabled, as in the Photoshop-style menus.

use crate::PhotocraftApp;
use crate::menus::MenuItem;

/// One row of a menu: `(command id, Russian label, English label)`, a separator, or a submenu.
pub(crate) enum Row {
    Item(&'static str, &'static str, &'static str),
    Sep,
    Sub(&'static str, &'static str, &'static [Row]),
}
use Row::{Item, Sep, Sub};

/// A top-level menu. `key` is the first element of its items' paths.
pub(crate) struct Top {
    pub key: &'static str,
    pub ru: &'static str,
    pub en: &'static str,
    pub rows: &'static [Row],
}

pub(crate) const TOPS: &[Top] = &[
    Top {
        key: "File",
        ru: "Файл",
        en: "File",
        rows: &[
            Item("file.new", "Создать...", "New..."),
            Item("file.open", "Открыть...", "Open..."),
            Sep,
            Item("file.close", "Закрыть", "Close"),
            Item("file.closeAll", "Закрыть все", "Close All"),
            Sep,
            Item("file.save", "Сохранить", "Save"),
            Item("file.saveAs", "Сохранить как...", "Save As..."),
            Item("file.saveACopy", "Сохранить копию...", "Save a Copy..."),
            Item("file.revert", "Вернуть к сохранённому", "Revert"),
            Item("pdn.snapshots", "Снимки работы...", "Work Snapshots..."),
            Sep,
            Sub(
                "Экспорт",
                "Export",
                &[
                    Item("file.export.quickExportAsPng", "Быстрый экспорт", "Quick Export"),
                    Item("file.export.exportAs", "Экспортировать как...", "Export As..."),
                    Item("file.export.layersToFiles", "Слои в файлы...", "Layers to Files..."),
                ],
            ),
            Item("pdn.importPdf", "Импорт PDF...", "Import PDF..."),
            Item("pdn.exportPdf", "Экспорт в PDF...", "Export to PDF..."),
            Item("pdn.pdfPages", "Страницы PDF", "PDF Pages"),
            Item("pdn.addRef", "Добавить референс...", "Add Reference..."),
            Item("file.placeEmbedded", "Поместить изображение...", "Place Image..."),
            Sep,
            Item("file.print", "Печать...", "Print..."),
            Sep,
            Item("file.exit", "Выход", "Exit"),
        ],
    },
    Top {
        key: "Edit",
        ru: "Правка",
        en: "Edit",
        rows: &[
            Item("edit.undo", "Отменить", "Undo"),
            Item("edit.redo", "Вернуть", "Redo"),
            Sep,
            Item("edit.cut", "Вырезать", "Cut"),
            Item("edit.copy", "Копировать", "Copy"),
            Item("edit.copyMerged", "Копировать (все слои)", "Copy Merged"),
            Item("edit.paste", "Вставить", "Paste"),
            Item("edit.pasteSpecial.pasteInPlace", "Вставить на то же место", "Paste in Place"),
            Item("file.newFromClipboard", "Вставить в новое изображение", "Paste Into New Image"),
            Sep,
            Item("edit.clear", "Очистить выделение", "Erase Selection"),
            Item("edit.fillForeground", "Заполнить выделение", "Fill Selection"),
            Item("select.inverse", "Обратить выделение", "Invert Selection"),
            Item("select.all", "Выделить все", "Select All"),
            Item("select.deselect", "Отменить выделение", "Deselect"),
            Sub(
                "Выделение",
                "Selection",
                &[
                    Item("select.modify.expand", "Расширить...", "Expand..."),
                    Item("select.modify.contract", "Сжать...", "Contract..."),
                    Item("select.modify.feather", "Растушевать...", "Feather..."),
                    Item("select.modify.smooth", "Сгладить...", "Smooth..."),
                    Item("select.modify.border", "Граница...", "Border..."),
                    Item("select.grow", "Расширить область", "Grow"),
                    Item("select.similar", "Подобные", "Similar"),
                    Item("select.colorRange", "По цвету...", "Color Range..."),
                ],
            ),
            Sep,
            Item("edit.freeTransform", "Свободное трансформирование", "Free Transform"),
            Item("edit.fill", "Заливка...", "Fill..."),
            Item("edit.stroke", "Обводка...", "Stroke..."),
            Sep,
            Item("edit.search", "Поиск команды...", "Search..."),
            Item("edit.keyboardShortcuts", "Сочетания клавиш...", "Keyboard Shortcuts..."),
            Item("edit.preferences.general", "Параметры...", "Settings..."),
        ],
    },
    Top {
        key: "View",
        ru: "Вид",
        en: "View",
        rows: &[
            Item("view.zoomIn", "Увеличить", "Zoom In"),
            Item("view.zoomOut", "Уменьшить", "Zoom Out"),
            Item("view.fitOnScreen", "По размеру окна", "Best Fit"),
            Item("view.actualPixels", "Истинный размер", "Actual Size"),
            Item("view.twoHundredPercent", "200%", "200%"),
            Sep,
            Item("pdn.toggleRefs", "Референсы", "References"),
            Item("view.rulers", "Линейки", "Rulers"),
            Item("view.show.grid", "Сетка", "Grid"),
            Item("view.show.pixelGrid", "Пиксельная сетка", "Pixel Grid"),
            Item("view.show.guides", "Направляющие", "Guides"),
            Item("view.snap", "Привязка", "Snap"),
            Item("view.show.selectionEdges", "Границы выделения", "Selection Edges"),
            Sep,
            Sub(
                "Режим экрана",
                "Screen Mode",
                &[
                    Item("view.screenMode.standard", "Обычный", "Standard"),
                    Item("view.screenMode.fullScreenWithMenuBar", "Во весь экран с меню", "Full Screen With Menu"),
                    Item("view.screenMode.fullScreen", "Во весь экран", "Full Screen"),
                ],
            ),
            Item("view.flipHorizontal", "Зеркально отразить вид", "Flip View"),
        ],
    },
    Top {
        key: "Image",
        ru: "Изображение",
        en: "Image",
        rows: &[
            Item("image.crop", "Обрезать по выделению", "Crop to Selection"),
            Item("image.trim", "Автоматическая обрезка...", "Auto-Crop..."),
            Sep,
            Item("image.imageSize", "Изменить размер...", "Resize..."),
            Item("image.canvasSize", "Размер полотна...", "Canvas Size..."),
            Sep,
            Item("image.imageRotation.flipCanvasHorizontal", "Перевернуть горизонтально", "Flip Horizontal"),
            Item("image.imageRotation.flipCanvasVertical", "Перевернуть вертикально", "Flip Vertical"),
            Item("image.imageRotation.90cw", "Повернуть по часовой стрелке на 90°", "Rotate 90° Clockwise"),
            Item("image.imageRotation.90ccw", "Повернуть против часовой стрелки на 90°", "Rotate 90° Counter-Clockwise"),
            Item("image.imageRotation.180", "Повернуть на 180°", "Rotate 180°"),
            Item("image.rotation.arbitrary", "Повернуть на угол...", "Rotate Arbitrary..."),
            Sep,
            Sub(
                "Режим",
                "Mode",
                &[
                    Item("image.mode.rgb", "RGB", "RGB"),
                    Item("image.mode.grayscale", "Градации серого", "Grayscale"),
                    Item("image.mode.cmyk", "CMYK", "CMYK"),
                    Item("image.mode.bits8", "8 бит на канал", "8 Bits/Channel"),
                    Item("image.mode.bits16", "16 бит на канал", "16 Bits/Channel"),
                ],
            ),
            Sep,
            Item("layer.flattenImage", "Объединить все слои", "Flatten"),
        ],
    },
    Top {
        key: "Layers",
        ru: "Слои",
        en: "Layers",
        rows: &[
            Item("layer.new.layer", "Добавить новый слой", "Add New Layer"),
            Item("layer.delete", "Удалить слой", "Delete Layer"),
            Item("layer.duplicate", "Создать копию слоя", "Duplicate Layer"),
            Item("layer.mergeDown", "Объединить со следующим слоем", "Merge Layer Down"),
            Sep,
            Item("file.placeEmbedded", "Импорт из файла...", "Import From File..."),
            Sep,
            Item("edit.transform.flipHorizontal", "Перевернуть горизонтально", "Flip Horizontal"),
            Item("edit.transform.flipVertical", "Перевернуть вертикально", "Flip Vertical"),
            Item("edit.transform.rotate180", "Повернуть на 180°", "Rotate 180°"),
            Item("edit.freeTransform", "Поворот / масштаб...", "Rotate / Zoom..."),
            Sep,
            Item("layer.arrange.bringToFront", "Переместить слой в верх", "Move Layer To Top"),
            Item("layer.arrange.bringForward", "Переместить слой вверх", "Move Layer Up"),
            Item("layer.arrange.sendBackward", "Переместить слой вниз", "Move Layer Down"),
            Item("layer.arrange.sendToBack", "Переместить слой в низ", "Move Layer To Bottom"),
            Sep,
            Item("layer.hideLayers", "Переключить видимость слоя", "Toggle Layer Visibility"),
            Item("pdn.layerProperties", "Свойства слоя...", "Layer Properties..."),
            Item("pdn.glow", "Свечение объекта...", "Object Glow..."),
        ],
    },
    Top {
        key: "Adjustments",
        ru: "Коррекция",
        en: "Adjustments",
        rows: &[
            Item("image.autoTone", "Автоуровни", "Auto-Level"),
            Item("image.adjustments.blackWhite", "Черно-белое", "Black and White"),
            Item("image.adjustments.brightnessContrast", "Яркость и контрастность", "Brightness / Contrast"),
            Item("image.adjustments.curves", "Кривые...", "Curves..."),
            Item("image.adjustments.exposure", "Экспозиция...", "Exposure..."),
            Item("image.adjustments.hueSaturation", "Оттенок и насыщенность...", "Hue / Saturation..."),
            Item("image.adjustments.invert", "Инвертировать цвета", "Invert Colors"),
            Item("image.adjustments.levels", "Уровни...", "Levels..."),
            Item("image.adjustments.posterize", "Постеризация...", "Posterize..."),
            Item("image.adjustments.shadowsHighlights", "Света и тени...", "Shadows / Highlights..."),
            Sep,
            Item("image.adjustments.colorBalance", "Цветовой баланс...", "Color Balance..."),
            Item("image.adjustments.vibrance", "Сочность...", "Vibrance..."),
            Item("image.adjustments.selectiveColor", "Выборочная коррекция цвета...", "Selective Color..."),
            Item("image.adjustments.channelMixer", "Микширование каналов...", "Channel Mixer..."),
            Item("image.adjustments.photoFilter", "Фотофильтр...", "Photo Filter..."),
            Item("image.adjustments.threshold", "Порог...", "Threshold..."),
            Item("image.adjustments.gradientMap", "Карта градиента...", "Gradient Map..."),
            Item("image.adjustments.replaceColor", "Заменить цвет...", "Replace Color..."),
            Item("image.adjustments.desaturate", "Обесцветить", "Desaturate"),
            Item("image.adjustments.equalize", "Выровнять яркость", "Equalize"),
            Item("image.autoContrast", "Автоконтраст", "Auto Contrast"),
            Item("image.autoColor", "Автоцвет", "Auto Color"),
        ],
    },
    Top {
        key: "Effects",
        ru: "Эффекты",
        en: "Effects",
        rows: &[
            Item("filter.lastFilter", "Повторить последний эффект", "Repeat Last Effect"),
            Sep,
            Sub(
                "Размытие",
                "Blurs",
                &[
                    Item("filter.blur.gaussianBlur", "Размытие по Гауссу...", "Gaussian Blur..."),
                    Item("filter.blur.motionBlur", "В движении...", "Motion Blur..."),
                    Item("filter.blur.radialBlur", "Круговое...", "Radial Blur..."),
                    Item("filter.blur.surfaceBlur", "Поверхностное размытие...", "Surface Blur..."),
                    Item("filter.blur.boxBlur", "Размытие рамкой...", "Box Blur..."),
                    Item("filter.blur.lensBlur", "Несфокусированность...", "Unfocus..."),
                    Item("filter.blur.smartBlur", "Умное размытие...", "Smart Blur..."),
                ],
            ),
            Sub(
                "Искажение",
                "Distort",
                &[
                    Item("filter.distort.spherize", "Выпуклость...", "Bulge..."),
                    Item("filter.distort.twirl", "Скручивание...", "Twist..."),
                    Item("filter.distort.ripple", "Рябь...", "Ripple..."),
                    Item("filter.distort.wave", "Волна...", "Wave..."),
                    Item("filter.distort.zigZag", "Зигзаг...", "Zigzag..."),
                    Item("filter.distort.shear", "Сдвиг...", "Shear..."),
                    Item("filter.distort.pinch", "Сжатие...", "Pinch..."),
                    Item("filter.distort.polarCoordinates", "Полярные координаты...", "Polar Coordinates..."),
                    Item("filter.pixelate.mosaic", "Укрупнение пикселей...", "Pixelate..."),
                    Item("filter.pixelate.crystallize", "Кристаллизация...", "Crystallize..."),
                    Item("filter.pixelate.fragment", "Фрагментация", "Fragment"),
                    Item("filter.pixelate.pointillize", "Пуантилизм...", "Pointillize..."),
                ],
            ),
            Sub(
                "Шум",
                "Noise",
                &[
                    Item("filter.noise.addNoise", "Добавление шума...", "Add Noise..."),
                    Item("filter.noise.median", "Медиана...", "Median..."),
                    Item("filter.noise.reduceNoise", "Уменьшение шума...", "Reduce Noise..."),
                    Item("filter.noise.despeckle", "Удаление крапинок", "Despeckle"),
                    Item("filter.noise.dustAndScratches", "Пыль и царапины...", "Dust and Scratches..."),
                ],
            ),
            Sub(
                "Для фотографий",
                "Photo",
                &[
                    Item("pdn.glow", "Свечение объекта...", "Object Glow..."),
                    Item("filter.sharpen.sharpen", "Резкость", "Sharpen"),
                    Item("filter.sharpen.unsharpMask", "Контурная резкость...", "Unsharp Mask..."),
                    Item("filter.sharpen.smartSharpen", "Умная резкость...", "Smart Sharpen..."),
                    Item("filter.lensCorrection", "Коррекция объектива...", "Lens Correction..."),
                    Item("filter.cameraRaw", "Camera Raw...", "Camera Raw..."),
                ],
            ),
            Sub(
                "Узоры",
                "Render",
                &[
                    Item("filter.render.clouds", "Облака", "Clouds"),
                    Item("filter.render.differenceClouds", "Разностные облака", "Difference Clouds"),
                    Item("filter.render.fibers", "Волокна...", "Fibers..."),
                    Item("filter.render.flame", "Пламя...", "Flame..."),
                    Item("filter.render.tree", "Дерево...", "Tree..."),
                    Item("filter.render.lensFlare", "Блик...", "Lens Flare..."),
                    Item("filter.render.lightingEffects", "Эффекты освещения...", "Lighting Effects..."),
                    Item("filter.render.pictureFrame", "Рамка...", "Picture Frame..."),
                ],
            ),
            Sub(
                "Стилизация",
                "Stylize",
                &[
                    Item("filter.stylize.emboss", "Барельеф...", "Emboss..."),
                    Item("filter.stylize.findEdges", "Подсвеченные края", "Edge Detect"),
                    Item("filter.stylize.oilPaint", "Картина маслом...", "Oil Painting..."),
                    Item("filter.stylize.traceContour", "Контур...", "Outline..."),
                    Item("filter.stylize.solarize", "Соляризация", "Solarize"),
                    Item("filter.stylize.tiles", "Плитка...", "Tiles..."),
                    Item("filter.stylize.wind", "Ветер...", "Wind..."),
                    Item("filter.stylize.diffuse", "Рассеивание...", "Diffuse..."),
                    Item("filter.stylize.extrude", "Выдавливание...", "Extrude..."),
                ],
            ),
            Sub(
                "Дополнительно",
                "Advanced",
                &[
                    Item("filter.liquify", "Пластика...", "Liquify..."),
                    Item("filter.filterGallery", "Галерея фильтров...", "Filter Gallery..."),
                    Item("filter.other.highPass", "Цветовой контраст...", "High Pass..."),
                    Item("filter.other.maximum", "Максимум...", "Maximum..."),
                    Item("filter.other.minimum", "Минимум...", "Minimum..."),
                    Item("filter.other.offset", "Сдвиг изображения...", "Offset..."),
                    Item("filter.other.custom", "Свой фильтр...", "Custom..."),
                ],
            ),
        ],
    },
];

fn russian() -> bool {
    crate::i18n::current().code() == "ru"
}

/// The label of a top-level menu in the UI language.
pub(crate) fn top_label(key: &str) -> String {
    match TOPS.iter().find(|t| t.key == key) {
        Some(t) if russian() => t.ru.to_string(),
        Some(t) => t.en.to_string(),
        None => key.to_string(),
    }
}

fn sep(path: &[String]) -> MenuItem {
    MenuItem { id: "---".into(), label: "---".into(), path: path.to_vec(), shortcut: None, enabled: false, checked: None, color: None }
}

fn push_rows(app: &PhotocraftApp, rows: &[Row], path: &mut Vec<String>, ru: bool, out: &mut Vec<MenuItem>) {
    for row in rows {
        match row {
            Row::Sep => {
                // A separator never opens or ends a menu, nor doubles.
                if out.last().is_some_and(|l| l.path.first() == path.first() && l.id != "---") {
                    out.push(sep(path));
                }
            }
            Row::Sub(r, e, children) => {
                path.push((if ru { *r } else { *e }).to_string());
                push_rows(app, children, path, ru, out);
                path.pop();
            }
            Row::Item(id, r, e) => out.push(MenuItem {
                id: (*id).to_string(),
                label: (if ru { *r } else { *e }).to_string(),
                path: path.clone(),
                shortcut: crate::shortcuts::effective_shortcut(app, id, crate::shortcuts::default_shortcut(id).as_deref()),
                enabled: crate::menus::is_live(id) && crate::menus::is_enabled(app, id),
                checked: crate::menus::checked(app, id),
                color: None,
            }),
        }
    }
}

/// Every menu row, in order, with paths starting at the top menu's key.
pub(crate) fn items(app: &PhotocraftApp) -> Vec<MenuItem> {
    let ru = russian();
    let mut out = Vec::new();
    for top in TOPS {
        let mut path = vec![top.key.to_string()];
        push_rows(app, top.rows, &mut path, ru, &mut out);
        // A trailing separator would end the menu on a line.
        if out.last().is_some_and(|l| l.id == "---") {
            out.pop();
        }
    }
    insert_recent(app, ru, &mut out);
    out
}

/// File › Open Recent (a submenu of the recently opened files) after Open.
fn insert_recent(app: &PhotocraftApp, ru: bool, items: &mut Vec<MenuItem>) {
    let Some(after) = items.iter().position(|i| i.id == "file.open") else { return };
    let path = vec!["File".to_string(), (if ru { "Недавние файлы" } else { "Open Recent" }).to_string()];
    let mut recent: Vec<MenuItem> = app
        .ui
        .recent_files
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let label = std::path::Path::new(p).file_name().map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned());
            MenuItem { id: format!("file.openRecent.{i}"), label, path: path.clone(), shortcut: None, enabled: true, checked: None, color: None }
        })
        .collect();
    if recent.is_empty() {
        recent.push(MenuItem {
            id: "file.clearRecent".into(),
            label: (if ru { "(нет)" } else { "(none)" }).into(),
            path: path.clone(),
            shortcut: None,
            enabled: false,
            checked: None,
            color: None,
        });
    } else {
        recent.push(sep(&path));
        recent.push(MenuItem {
            id: "file.clearRecent".into(),
            label: (if ru { "Очистить этот список" } else { "Clear this list" }).into(),
            path,
            shortcut: None,
            enabled: true,
            checked: None,
            color: None,
        });
    }
    for (k, it) in recent.into_iter().enumerate() {
        items.insert(after + 1 + k, it);
    }
}
