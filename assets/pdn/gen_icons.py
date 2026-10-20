#!/usr/bin/env python3
"""Генератор цветных иконок в стиле Paint.NET для форка PhotoCraft.

Запуск:  python gen_icons.py <каталог-вывода>
         (для форка: ...\\photocraft-ui\\src\\assets\\pdn)

Каждая иконка — 24x24 SVG, плоские цвета. Рисуются руками (оригинальный рисунок, не копия
иконок Paint.NET). Имена файлов используются в `crates/ui-egui/src/pdn/icons.rs`.
"""
import os
import sys

# палитра
BLUE = "#2f7fd6"
LBLUE = "#8cc4f5"
DBLUE = "#1b4f8f"
RED = "#d9453b"
ORANGE = "#f1a22c"
YELLOW = "#f6d33c"
GREEN = "#43a64d"
GRAY = "#8a8f98"
LGRAY = "#d5d9df"
DGRAY = "#4a4f57"
WHITE = "#ffffff"
BROWN = "#a9713e"
BLACK = "#22262b"
PINK = "#e35d9a"
PURPLE = "#8c55c8"

ICONS = {}


def icon(name, body):
    ICONS[name] = body


def doc(fill=WHITE, extra=""):
    # лист с загнутым углом
    return (f'<path d="M5 2h9l5 5v15H5z" fill="{fill}" stroke="{DGRAY}" stroke-width="1.2" stroke-linejoin="round"/>'
            f'<path d="M14 2v5h5" fill="{LGRAY}" stroke="{DGRAY}" stroke-width="1.2" stroke-linejoin="round"/>{extra}')


# ---------------------------------------------------------------- панель команд
icon("new", doc(extra=f'<circle cx="17" cy="17" r="5.2" fill="{GREEN}"/><path d="M17 14v6M14 17h6" stroke="{WHITE}" stroke-width="1.8" stroke-linecap="round"/>'))
icon("open", f'<path d="M2 6h7l2 2h9v3H6l-3 9z" fill="{ORANGE}" stroke="{BROWN}" stroke-width="1.1" stroke-linejoin="round"/>'
             f'<path d="M6 11h17l-3.5 9H3z" fill="{YELLOW}" stroke="{BROWN}" stroke-width="1.1" stroke-linejoin="round"/>')
icon("save", f'<path d="M3 3h15l3 3v15H3z" fill="{BLUE}" stroke="{DBLUE}" stroke-width="1.2" stroke-linejoin="round"/>'
             f'<rect x="7" y="3" width="9" height="6" fill="{LGRAY}"/><rect x="13" y="4" width="2" height="4" fill="{DGRAY}"/>'
             f'<rect x="6" y="13" width="12" height="8" rx="1" fill="{WHITE}"/>')
icon("print", f'<rect x="7" y="3" width="10" height="6" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.1"/>'
              f'<rect x="3" y="9" width="18" height="9" rx="2" fill="{GRAY}" stroke="{DGRAY}" stroke-width="1.1"/>'
              f'<rect x="7" y="14" width="10" height="7" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.1"/>'
              f'<circle cx="18" cy="12" r="1" fill="{GREEN}"/>')
icon("cut", f'<path d="M8 3l6 11M16 3l-6 11" stroke="{GRAY}" stroke-width="2" stroke-linecap="round"/>'
            f'<circle cx="8" cy="18" r="3.2" fill="none" stroke="{RED}" stroke-width="2"/><circle cx="16" cy="18" r="3.2" fill="none" stroke="{RED}" stroke-width="2"/>')
icon("copy", f'<path d="M4 3h8l3 3v11H4z" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2" stroke-linejoin="round"/>'
             f'<path d="M9 8h8l3 3v11H9z" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2" stroke-linejoin="round"/>'
             f'<path d="M11.5 14h5M11.5 17h5" stroke="{BLUE}" stroke-width="1.3" stroke-linecap="round"/>')
icon("paste", f'<rect x="3" y="4" width="14" height="17" rx="1.5" fill="{ORANGE}" stroke="{BROWN}" stroke-width="1.2"/>'
              f'<rect x="7" y="2" width="6" height="4" rx="1" fill="{LGRAY}" stroke="{DGRAY}" stroke-width="1.1"/>'
              f'<path d="M11 10h6l4 4v8H11z" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2" stroke-linejoin="round"/>')
icon("crop", f'<path d="M7 2v15h15M2 7h15v15" fill="none" stroke="{DGRAY}" stroke-width="2.4" stroke-linejoin="miter"/>'
             f'<rect x="9" y="9" width="6" height="6" fill="{LBLUE}" opacity="0.8"/>')
icon("deselect", f'<rect x="3.5" y="3.5" width="17" height="17" fill="none" stroke="{DGRAY}" stroke-width="1.6" stroke-dasharray="3 2.2"/>'
                 f'<path d="M8 8l8 8M16 8l-8 8" stroke="{RED}" stroke-width="2.4" stroke-linecap="round"/>')
icon("undo", f'<path d="M9 5L3.5 10 9 15" fill="none" stroke="{BLUE}" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>'
             f'<path d="M4 10h9a6 6 0 010 12h-3" fill="none" stroke="{BLUE}" stroke-width="2.4" stroke-linecap="round"/>')
icon("redo", f'<path d="M15 5l5.5 5L15 15" fill="none" stroke="{BLUE}" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>'
             f'<path d="M20 10h-9a6 6 0 000 12h3" fill="none" stroke="{BLUE}" stroke-width="2.4" stroke-linecap="round"/>')
icon("grid", f'<rect x="3" y="3" width="18" height="18" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/>'
             f'<path d="M9 3v18M15 3v18M3 9h18M3 15h18" stroke="{BLUE}" stroke-width="1.2"/>')
icon("rulers", f'<rect x="2" y="7" width="20" height="10" rx="1" fill="{YELLOW}" stroke="{BROWN}" stroke-width="1.2"/>'
               f'<path d="M6 7v4M10 7v6M14 7v4M18 7v6" stroke="{BROWN}" stroke-width="1.3"/>')

# ---------------------------------------------------------------- окна
icon("win-tools", f'<path d="M4 20L14 10" stroke="{BROWN}" stroke-width="3.2" stroke-linecap="round"/>'
                  f'<path d="M12 4l4-2 6 6-2 4-3-1-3-3z" fill="{GRAY}" stroke="{DGRAY}" stroke-width="1.1" stroke-linejoin="round"/>')
icon("win-history", f'<circle cx="12" cy="12" r="9.5" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.4"/>'
                    f'<path d="M12 6v6l4 3" fill="none" stroke="{BLUE}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>')
icon("win-layers", f'<rect x="3" y="3" width="13" height="13" fill="{LBLUE}" stroke="{DBLUE}" stroke-width="1.2"/>'
                   f'<rect x="8" y="8" width="13" height="13" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/>')
icon("win-colors", f'<circle cx="12" cy="12" r="9.5" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1"/>'
                   f'<path d="M12 12V2.5A9.5 9.5 0 0120.2 7.2z" fill="{RED}"/><path d="M12 12l8.2-4.8A9.5 9.5 0 0120.2 16.8z" fill="{YELLOW}"/>'
                   f'<path d="M12 12l8.2 4.8A9.5 9.5 0 0112 21.5z" fill="{GREEN}"/><path d="M12 12v9.5A9.5 9.5 0 013.8 16.8z" fill="{BLUE}"/>'
                   f'<path d="M12 12l-8.2 4.8A9.5 9.5 0 013.8 7.2z" fill="{PURPLE}"/><path d="M12 12L3.8 7.2A9.5 9.5 0 0112 2.5z" fill="{PINK}"/>'
                   f'<circle cx="12" cy="12" r="3.2" fill="{WHITE}"/>')
icon("gear", f'<circle cx="12" cy="12" r="3.4" fill="none" stroke="{DGRAY}" stroke-width="2"/>'
             f'<path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2" stroke="{DGRAY}" stroke-width="2.4" stroke-linecap="round"/>'
             f'<circle cx="12" cy="12" r="6.8" fill="none" stroke="{DGRAY}" stroke-width="2"/>')
icon("help", f'<circle cx="12" cy="12" r="10" fill="{BLUE}"/><path d="M8.6 9.2a3.4 3.4 0 116 2c-1.3 1-2.6 1.6-2.6 3.2" fill="none" stroke="{WHITE}" stroke-width="2.2" stroke-linecap="round"/>'
             f'<circle cx="12" cy="18" r="1.4" fill="{WHITE}"/>')

# ---------------------------------------------------------------- инструменты
icon("rect-select", f'<rect x="3.5" y="5.5" width="17" height="13" fill="{LBLUE}" fill-opacity="0.55" stroke="{DBLUE}" stroke-width="1.6" stroke-dasharray="3.2 2.2"/>')
icon("ellipse-select", f'<ellipse cx="12" cy="12" rx="9" ry="7" fill="{LBLUE}" fill-opacity="0.55" stroke="{DBLUE}" stroke-width="1.6" stroke-dasharray="3.2 2.2"/>')
icon("lasso", f'<path d="M12 4C6 4 3 7 4 10s5 3 8 3 7-1 8-4-3-5-8-5z" fill="{LBLUE}" fill-opacity="0.5" stroke="{DBLUE}" stroke-width="1.6" stroke-dasharray="3 2"/>'
              f'<path d="M9 14c-1 3 0 5-3 6" fill="none" stroke="{DGRAY}" stroke-width="1.6" stroke-linecap="round"/>')
icon("move-pixels", f'<path d="M12 2l3.2 3.6h-2.2v5.4h5.4V8.8L22 12l-3.6 3.2v-2.2H13v5.4h2.2L12 22l-3.2-3.6h2.2V13H5.6v2.2L2 12l3.6-3.2V11H11V5.6H8.8z" fill="{DBLUE}" stroke="{WHITE}" stroke-width="0.8" stroke-linejoin="round"/>')
icon("move-selection", f'<rect x="3.5" y="3.5" width="12" height="10" fill="{LBLUE}" fill-opacity="0.45" stroke="{DBLUE}" stroke-width="1.5" stroke-dasharray="3 2"/>'
                       f'<path d="M12 11l2.4 9 2.2-3.6L20 18l-1.6-3.4L22 13.8z" fill="{WHITE}" stroke="{BLACK}" stroke-width="1.2" stroke-linejoin="round"/>')
icon("zoom", f'<circle cx="10" cy="10" r="6.6" fill="{LBLUE}" fill-opacity="0.45" stroke="{DGRAY}" stroke-width="2"/>'
             f'<path d="M15 15l6.5 6.5" stroke="{BROWN}" stroke-width="3.4" stroke-linecap="round"/>'
             f'<path d="M7 10h6M10 7v6" stroke="{DBLUE}" stroke-width="1.8" stroke-linecap="round"/>')
icon("wand", f'<path d="M4 20L15 9" stroke="{BROWN}" stroke-width="3" stroke-linecap="round"/>'
             f'<path d="M17 3l1 2.4 2.4 1-2.4 1L17 10l-1-2.6-2.4-1 2.4-1z" fill="{YELLOW}" stroke="{ORANGE}" stroke-width="0.8" stroke-linejoin="round"/>'
             f'<circle cx="20" cy="13" r="1.1" fill="{YELLOW}"/><circle cx="7" cy="7" r="1.1" fill="{YELLOW}"/>')
icon("pan", f'<path d="M8 21c-3-2-5-5-5-8l1.5-1.3L7 14V5a1.4 1.4 0 012.8 0v6h.4V3.5a1.4 1.4 0 012.8 0V11h.4V4.6a1.4 1.4 0 012.8 0V11h.4V7a1.4 1.4 0 012.8 0v7c0 4-2.5 7-6 7z" fill="#f5cfa0" stroke="{BROWN}" stroke-width="1.2" stroke-linejoin="round"/>')
icon("bucket", f'<path d="M4 12l8-8 8 8-7 7a3 3 0 01-4.2 0z" fill="{LBLUE}" stroke="{DBLUE}" stroke-width="1.4" stroke-linejoin="round"/>'
               f'<path d="M12 4L9 1.5" stroke="{DGRAY}" stroke-width="1.6" stroke-linecap="round"/>'
               f'<path d="M21 14c1.6 2.2 2 3.2 2 4.2a2 2 0 01-4 0c0-1 .4-2 2-4.2z" fill="{BLUE}"/>')
icon("gradient", f'<defs><linearGradient id="g" x1="0" x2="1"><stop offset="0" stop-color="{BLACK}"/><stop offset="1" stop-color="{WHITE}"/></linearGradient></defs>'
                 f'<rect x="3" y="5" width="18" height="14" fill="url(#g)" stroke="{DGRAY}" stroke-width="1.3"/>')
icon("brush", f'<path d="M20 3c-4 1-8 5-10 9l2.6 2.6C16.5 12.6 20 8 20 3z" fill="{BROWN}" stroke="{BLACK}" stroke-width="1" stroke-linejoin="round"/>'
              f'<path d="M9 13c-3 0-4 2-4.4 4.4C4 20 3 21 2.4 21.6c3 1 6 .6 8-1.4 1.2-1.2 1.6-2.6 0-4z" fill="{BLUE}" stroke="{DBLUE}" stroke-width="1"/>')
icon("eraser", f'<path d="M3 15l9-10 9 8-6 7H8z" fill="{PINK}" stroke="{DGRAY}" stroke-width="1.3" stroke-linejoin="round"/>'
               f'<path d="M8 20l-5-5 9-10 5 4.5" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.3" stroke-linejoin="round"/>'
               f'<path d="M8 20h13" stroke="{DGRAY}" stroke-width="1.6" stroke-linecap="round"/>')
icon("pencil", f'<path d="M4 20l1-5L17 3l4 4L9 19z" fill="{YELLOW}" stroke="{BROWN}" stroke-width="1.3" stroke-linejoin="round"/>'
               f'<path d="M4 20l1-5 4 4z" fill="#f5cfa0" stroke="{BROWN}" stroke-width="1.1" stroke-linejoin="round"/>'
               f'<path d="M4 20l.6-2.6 2 2z" fill="{BLACK}"/><path d="M15 5l4 4" stroke="{PINK}" stroke-width="2.6"/>')
icon("picker", f'<path d="M3 21l1-4 9-9 3 3-9 9z" fill="{LGRAY}" stroke="{DGRAY}" stroke-width="1.3" stroke-linejoin="round"/>'
               f'<path d="M12.5 6.5l5 5" stroke="{DGRAY}" stroke-width="2.2"/><path d="M16 3l5 5-2.2 2.2-5-5z" fill="{BLUE}" stroke="{DBLUE}" stroke-width="1.1" stroke-linejoin="round"/>'
               f'<path d="M3 21l1-4 3 3z" fill="{BLUE}"/>')
icon("clone", f'<path d="M9 14h6v3a1 1 0 01-1 1H10a1 1 0 01-1-1z" fill="{BROWN}"/>'
              f'<path d="M12 3c2 0 2.4 2 1.6 4.4L14 12h-4l.4-4.6C9.6 5 10 3 12 3z" fill="{ORANGE}" stroke="{BROWN}" stroke-width="1.1" stroke-linejoin="round"/>'
              f'<path d="M4 18h16a1 1 0 011 1v2H3v-2a1 1 0 011-1z" fill="{RED}" stroke="{BROWN}" stroke-width="1.1" stroke-linejoin="round"/>')
icon("recolor", f'<path d="M6 21l-3-3 11-11 4 4z" fill="{LGRAY}" stroke="{DGRAY}" stroke-width="1.2" stroke-linejoin="round"/>'
                f'<path d="M14 7l3-3.5L21 7l-3 3.5z" fill="{GREEN}" stroke="{DGRAY}" stroke-width="1.1" stroke-linejoin="round"/>'
                f'<circle cx="19" cy="18" r="3.4" fill="{RED}"/><circle cx="16" cy="20.4" r="2.6" fill="{BLUE}" opacity="0.9"/>')
icon("text", f'<path d="M5 5h14M12 5v15M9 20h6" fill="none" stroke="{DGRAY}" stroke-width="2.6" stroke-linecap="square"/>'
             f'<path d="M5 5v3M19 5v3" stroke="{DGRAY}" stroke-width="2.6"/>')
icon("line", f'<path d="M3 20C8 20 6 4 12 4s4 16 9 16" fill="none" stroke="{DGRAY}" stroke-width="2.4" stroke-linecap="round"/>'
             f'<rect x="1.6" y="18.4" width="3.4" height="3.4" fill="{WHITE}" stroke="{DBLUE}" stroke-width="1.1"/>'
             f'<rect x="19" y="18.4" width="3.4" height="3.4" fill="{WHITE}" stroke="{DBLUE}" stroke-width="1.1"/>'
             f'<rect x="10.3" y="2.3" width="3.4" height="3.4" fill="{WHITE}" stroke="{DBLUE}" stroke-width="1.1"/>')
icon("shapes", f'<rect x="2.5" y="4" width="11" height="11" fill="{BLUE}" stroke="{DBLUE}" stroke-width="1.2"/>'
               f'<circle cx="15" cy="15" r="5.6" fill="{PURPLE}" stroke="{WHITE}" stroke-width="1.2"/>'
               f'<path d="M14.5 7l5.5 9h-11z" fill="{GREEN}" stroke="{WHITE}" stroke-width="1.2" stroke-linejoin="round" opacity="0.95"/>')
icon("pen", f'<path d="M12 3l5 6-3 10h-4L7 9z" fill="{LGRAY}" stroke="{DGRAY}" stroke-width="1.3" stroke-linejoin="round"/>'
            f'<circle cx="12" cy="11" r="1.6" fill="{DGRAY}"/><path d="M12 12.6V19" stroke="{DGRAY}" stroke-width="1.3"/><rect x="8" y="19" width="8" height="2.6" fill="{BLUE}"/>')
icon("hand-flip", f'<path d="M3 12h18M7 8l-4 4 4 4M17 8l4 4-4 4" fill="none" stroke="{BLUE}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>')
icon("healing", f'<rect x="2" y="9" width="20" height="7" rx="3.5" fill="#f5cfa0" stroke="{BROWN}" stroke-width="1.2" transform="rotate(-35 12 12.5)"/>'
                f'<circle cx="12" cy="12.5" r="1.1" fill="{BROWN}"/><circle cx="9" cy="14.5" r="1" fill="{BROWN}"/><circle cx="15" cy="10.5" r="1" fill="{BROWN}"/>')

# ---------------------------------------------------------------- слои и прочее
icon("layer-add", f'<rect x="3" y="5" width="15" height="15" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/><circle cx="17.5" cy="7" r="5" fill="{GREEN}"/>'
                  f'<path d="M17.5 4.2v5.6M14.7 7h5.6" stroke="{WHITE}" stroke-width="1.8" stroke-linecap="round"/>')
icon("layer-del", f'<rect x="3" y="5" width="15" height="15" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/><circle cx="17.5" cy="7" r="5" fill="{RED}"/>'
                  f'<path d="M15 4.5l5 5M20 4.5l-5 5" stroke="{WHITE}" stroke-width="1.8" stroke-linecap="round"/>')
icon("layer-dup", f'<rect x="3" y="3" width="13" height="13" fill="{LBLUE}" stroke="{DBLUE}" stroke-width="1.2"/><rect x="8" y="8" width="13" height="13" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/>'
                  f'<path d="M14.5 11v7M11 14.5h7" stroke="{GREEN}" stroke-width="1.8" stroke-linecap="round"/>')
icon("layer-merge", f'<rect x="5" y="3" width="14" height="8" fill="{LBLUE}" stroke="{DBLUE}" stroke-width="1.2"/><rect x="5" y="13" width="14" height="8" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/>'
                    f'<path d="M12 8v8M9 13l3 3 3-3" fill="none" stroke="{DBLUE}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>')
icon("layer-up", f'<rect x="4" y="9" width="16" height="12" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/><path d="M12 2v10M7.5 6.5L12 2l4.5 4.5" fill="none" stroke="{GREEN}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>')
icon("layer-down", f'<rect x="4" y="3" width="16" height="12" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/><path d="M12 22V12M7.5 17.5L12 22l4.5-4.5" fill="none" stroke="{BLUE}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>')
icon("layer-props", f'<rect x="3" y="3" width="15" height="15" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.2"/>'
                    f'<path d="M12 21l-.5-3.5 7-7 3 3-7 7z" fill="{BLUE}" stroke="{DBLUE}" stroke-width="1" stroke-linejoin="round"/>')
icon("eye", f'<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z" fill="{WHITE}" stroke="{DGRAY}" stroke-width="1.4"/><circle cx="12" cy="12" r="3.4" fill="{BLUE}"/><circle cx="12" cy="12" r="1.4" fill="{BLACK}"/>')
icon("zoom-in", f'<circle cx="10" cy="10" r="6.4" fill="none" stroke="{DGRAY}" stroke-width="1.8"/><path d="M15 15l6 6" stroke="{BROWN}" stroke-width="3" stroke-linecap="round"/><path d="M7 10h6M10 7v6" stroke="{GREEN}" stroke-width="1.8" stroke-linecap="round"/>')
icon("zoom-out", f'<circle cx="10" cy="10" r="6.4" fill="none" stroke="{DGRAY}" stroke-width="1.8"/><path d="M15 15l6 6" stroke="{BROWN}" stroke-width="3" stroke-linecap="round"/><path d="M7 10h6" stroke="{RED}" stroke-width="1.8" stroke-linecap="round"/>')
icon("fit", f'<rect x="3" y="5" width="18" height="14" fill="none" stroke="{DGRAY}" stroke-width="1.5"/><path d="M7 9l-2 2 2 2M17 9l2 2-2 2M8 11h8" fill="none" stroke="{BLUE}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>')
icon("size", f'<rect x="3" y="6" width="18" height="12" fill="{LBLUE}" fill-opacity="0.5" stroke="{DBLUE}" stroke-width="1.4"/><path d="M3 21h18M3 19.5v3M21 19.5v3" stroke="{DGRAY}" stroke-width="1.3"/>')
icon("cursor", f'<path d="M5 3l14 8-6 1.4L10 19z" fill="{WHITE}" stroke="{BLACK}" stroke-width="1.4" stroke-linejoin="round"/>')
icon("close", f'<path d="M6 6l12 12M18 6L6 18" stroke="{WHITE}" stroke-width="3" stroke-linecap="round"/>')
icon("plus", f'<path d="M12 5v14M5 12h14" stroke="{WHITE}" stroke-width="3" stroke-linecap="round"/>')


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "pdn-icons"
    os.makedirs(out, exist_ok=True)
    for name, body in ICONS.items():
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24">{body}</svg>\n'
        with open(os.path.join(out, name + ".svg"), "w", encoding="utf-8", newline="\n") as f:
            f.write(svg)
    # таблица для include_bytes! (Rust)
    with open(os.path.join(out, "table.rs.txt"), "w", encoding="utf-8", newline="\n") as f:
        for name in ICONS:
            f.write(f'    ("{name}", include_bytes!("../../../../assets/pdn/{name}.svg")),\n')
    print(len(ICONS), "icons ->", out)


if __name__ == "__main__":
    main()
