// Copyright 2018 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The main edit view.

use std::cmp::min;
use std::ops::Range;

use serde_json::Value;

use winapi::um::winuser::*;

use direct2d::brush;
use direct2d::math::*;
use directwrite::{self, TextFormat, TextLayout};
use directwrite::text_format;
use directwrite::text_layout;

use xi_win_shell::paint::PaintCtx;
use xi_win_shell::util::default_text_options;
use xi_win_shell::window::{M_ALT, M_CTRL, M_SHIFT};

use MainWin;

use linecache::LineCache;

/// State and behavior for one editor view.
pub struct EditView {
    // Note: these public fields should be properly encapsulated.
    pub view_id: String,
    pub filename: Option<String>,
    line_cache: LineCache,
    dwrite_factory: directwrite::Factory,
    resources: Option<Resources>,
    scroll_offset: f32,
    size: (f32, f32),  // in px units
    viewport: Range<usize>,
}

struct Resources {
    fg: brush::SolidColor,
    bg: brush::SolidColor,
    text_format: TextFormat,
}

const TOP_PAD: f32 = 6.0;
const LINE_SPACE: f32 = 17.0;

impl EditView {
    pub fn new() -> EditView {
        EditView {
            view_id: "".into(),
            filename: None,
            line_cache: LineCache::new(),
            dwrite_factory: directwrite::Factory::new().unwrap(),
            resources: None,
            scroll_offset: 0.0,
            size: (0.0, 0.0),
            viewport: 0..0,
        }
    }

    fn create_resources(&mut self, p: &mut PaintCtx) -> Resources {
        let rt = p.render_target();
        let text_format_params = text_format::ParamBuilder::new()
            .size(15.0)
            .family("Consolas")
            .build().unwrap();
        let text_format = self.dwrite_factory.create(text_format_params).unwrap();
        Resources {
            fg: rt.create_solid_color_brush(0xf0f0ea, &BrushProperties::default()).unwrap(),
            bg: rt.create_solid_color_brush(0x272822, &BrushProperties::default()).unwrap(),
            text_format: text_format,
        }
    }

    pub fn rebuild_resources(&mut self) {
        self.resources = None;
    }

    pub fn size(&mut self, x: f32, y: f32) {
        self.size = (x, y);
        self.constrain_scroll();
    }

    pub fn clear_line_cache(&mut self) {
        self.line_cache = LineCache::new();
    }

    pub fn render(&mut self, p: &mut PaintCtx) {
        if self.resources.is_none() {
            self.resources = Some(self.create_resources(p));
        }
        let resources = &self.resources.as_ref().unwrap();
        let rt = p.render_target();
        let rect = RectF::from((0.0, 0.0, self.size.0, self.size.1));
        rt.fill_rectangle(&rect, &resources.bg);

        let first_line = self.y_to_line(0.0);
        let last_line = min(self.y_to_line(self.size.1) + 1, self.line_cache.height());

        let x0 = 6.0;
        let mut y = self.line_to_content_y(first_line) - self.scroll_offset;
        for line_num in first_line..last_line {
            if let Some(line) = self.line_cache.get_line(line_num) {
                let layout = resources.create_text_layout(&self.dwrite_factory, line.text());
                rt.draw_text_layout(
                    &Point2F::from((x0, y)),
                    &layout,
                    &resources.fg,
                    default_text_options()
                );
                for &offset in line.cursor() {
                    if let Some(pos) = layout.hit_test_text_position(offset as u32, true) {
                        let x = x0 + pos.point_x;
                        rt.draw_line(&Point2F::from((x, y)),
                            &Point2F::from((x, y + 17.0)),
                            &resources.fg, 1.0, None);
                    }
                }
            }
            y += LINE_SPACE;
        }
    }

    pub fn set_view_id(&mut self, view_id: &str) {
        self.view_id = view_id.into();
    }

    pub fn apply_update(&mut self, update: &Value) {
        self.line_cache.apply_update(update);
        self.constrain_scroll();
    }

    pub fn char(&self, ch: u32, _mods: u32, win: &MainWin) {
        let view_id = &self.view_id;
        if let Some(c) = ::std::char::from_u32(ch) {
            if ch >= 0x20 {
                // Don't insert control characters
                let params = json!({"chars": c.to_string()});
                win.send_edit_cmd("insert", &params, view_id);
            }
        }
    }

    /// Sends a simple action with no parameters
    fn send_action(&self, method: &str, win: &MainWin) {
        win.send_edit_cmd(method, &json!([]), &self.view_id);
    }

    pub fn keydown(&mut self, vk_code: i32, mods: u32, win: &MainWin) -> bool {
        // Handle special keys here
        match vk_code {
            VK_RETURN => {
                // TODO: modifiers are variants of open
                self.send_action("insert_newline", win);
            }
            VK_TAB => {
                // TODO: modified versions
                self.send_action("insert_tab", win);
            }
            VK_UP => {
                if mods == M_CTRL {
                    self.scroll_offset -= LINE_SPACE;
                    self.constrain_scroll();
                    self.update_viewport(win);
                    win.invalidate();
                } else {
                    let action = if mods == M_CTRL | M_ALT {
                        "add_selection_above"
                    } else {
                        s(mods, "move_up", "move_up_and_modify_selection")
                    };
                    // TODO: swap line up is ctrl + shift
                    self.send_action(action, win);
                }
            }
            VK_DOWN => {
                if mods == M_CTRL {
                    self.scroll_offset += LINE_SPACE;
                    self.constrain_scroll();
                    self.update_viewport(win);
                    win.invalidate();
                } else {
                    let action = if mods == M_CTRL | M_ALT {
                        "add_selection_below"
                    } else {
                        s(mods, "move_down", "move_down_and_modify_selection")
                    };
                    self.send_action(action, win);
                }
            }
            VK_LEFT => {
                // TODO: there is a subtle distinction between alt and ctrl
                let action = if (mods & (M_ALT | M_CTRL)) != 0 {
                    s(mods, "move_word_left", "move_word_left_and_modify_selection")
                } else {
                    s(mods, "move_left", "move_left_and_modify_selection")
                };
                self.send_action(action, win);
            }
            VK_RIGHT => {
                // TODO: there is a subtle distinction between alt and ctrl
                let action = if (mods & (M_ALT | M_CTRL)) != 0 {
                    s(mods, "move_word_right", "move_word_right_and_modify_selection")
                } else {
                    s(mods, "move_right", "move_right_and_modify_selection")
                };
                self.send_action(action, win);
            }
            VK_PRIOR => {
                self.send_action(s(mods, "scroll_page_up",
                    "page_up_and_modify_selection"), win);
            }
            VK_NEXT => {
                self.send_action(s(mods, "scroll_page_down",
                    "page_down_and_modify_selection"), win);
            }
            VK_HOME => {
                let action = if (mods & M_CTRL) != 0 {
                    s(mods, "move_to_beginning_of_document",
                        "move_to_beginning_of_document_and_modify_selection")
                } else {
                    s(mods, "move_to_left_end_of_line",
                        "move_to_left_end_of_line_and_modify_selection")
                };
                self.send_action(action, win);
            }
            VK_END => {
                let action = if (mods & M_CTRL) != 0 {
                    s(mods, "move_to_end_of_document",
                        "move_to_end_of_document_and_modify_selection")
                } else {
                    s(mods, "move_to_right_end_of_line",
                        "move_to_right_end_of_line_and_modify_selection")
                };
                self.send_action(action, win);
            }
            VK_ESCAPE => {
                self.send_action("cancel_operation", win);
            }
            VK_BACK => {
                let action = if (mods & M_CTRL) != 0 {
                    // should be "delete to beginning of paragraph" but not supported
                    s(mods, "delete_word_backward", "delete_to_beginning_of_line")
                } else {
                    "delete_backward"
                };
                self.send_action(action, win);
                self.send_action("delete_forward", win);
            }
            VK_DELETE => {
                let action = if (mods & M_CTRL) != 0 {
                    s(mods, "delete_word_forward", "delete_to_end_of_paragraph")
                } else {
                    // TODO: shift-delete should be "delete line"
                    "delete_forward"
                };
                self.send_action(action, win);
                self.send_action("delete_forward", win);
            }
            VK_OEM_4 => {
                // generally '[' key, but might vary on non-US keyboards
                if mods == M_CTRL {
                    self.send_action("outdent", win);
                } else {
                    return false
                }
            }
            VK_OEM_6 => {
                // generally ']' key, but might vary on non-US keyboards
                if mods == M_CTRL {
                    self.send_action("indent", win);
                } else {
                    return false
                }
            }
            _ => {
                return false
            }
        }
        true
    }

    // Commands

    pub fn undo(&mut self, win: &MainWin) {
        self.send_action("undo", win);
    }

    pub fn redo(&mut self, win: &MainWin) {
        self.send_action("redo", win);
    }

    pub fn upper_case(&mut self, win: &MainWin) {
        self.send_action("uppercase", win);
    }

    pub fn lower_case(&mut self, win: &MainWin) {
        self.send_action("lowercase", win);
    }

    pub fn transpose(&mut self, win: &MainWin) {
        self.send_action("transpose", win);
    }

    pub fn add_cursor_above(&mut self, win: &MainWin) {
        // Note: some subtlety around find, the escape key cancels it, but the menu
        // shouldn't.
        self.send_action("add_selection_above", win);
    }

    pub fn add_cursor_below(&mut self, win: &MainWin) {
        // Note: some subtlety around find, the escape key cancels it, but the menu
        // shouldn't.
        self.send_action("add_selection_below", win);
    }

    pub fn single_selection(&mut self, win: &MainWin) {
        // Note: some subtlety around find, the escape key cancels it, but the menu
        // shouldn't.
        self.send_action("cancel_operation", win);
    }

    pub fn select_all(&mut self, win: &MainWin) {
        // Note: some subtlety around find, the escape key cancels it, but the menu
        // shouldn't.
        self.send_action("select_all", win);
    }

    pub fn mouse_wheel(&mut self, delta: i32, _mods: u32, win: &MainWin) {
        // TODO: scale properly, taking SPI_GETWHEELSCROLLLINES into account
        let scroll_scaling = 0.5;
        self.scroll_offset -= (delta as f32) * scroll_scaling;
        self.constrain_scroll();
        self.update_viewport(win);
        win.handle.borrow().invalidate();
    }

    fn constrain_scroll(&mut self) {
        let max_scroll = TOP_PAD + LINE_SPACE *
            (self.line_cache.height().saturating_sub(1)) as f32;
        if self.scroll_offset < 0.0 {
            self.scroll_offset = 0.0;
        } else if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    // Takes y in screen-space px.
    fn y_to_line(&self, y: f32) -> usize {
        let mut line = (y + self.scroll_offset - TOP_PAD) / LINE_SPACE;
        if line < 0.0 { line = 0.0; }
        let line = line.floor() as usize;
        min(line, self.line_cache.height())
    }

    /// Convert line number to y coordinate in content space.
    fn line_to_content_y(&self, line: usize) -> f32 {
        TOP_PAD + (line as f32) * LINE_SPACE
    }

    fn update_viewport(&mut self, win: &MainWin) {
        let first_line = self.y_to_line(0.0);
        let last_line = first_line + ((self.size.1 / LINE_SPACE).floor() as usize) + 1;
        let viewport = first_line..last_line;
        if viewport != self.viewport {
            self.viewport = viewport;
            let view_id = &self.view_id;
            win.send_edit_cmd("scroll", &json!([first_line, last_line]), view_id);
        }
    }

    pub fn scroll_to(&mut self, line: usize) {
        let y = self.line_to_content_y(line);
        let bottom_slop = 20.0;
        if y < self.scroll_offset {
            self.scroll_offset = y;
        } else if y > self.scroll_offset + self.size.1 - bottom_slop {
            self.scroll_offset = y - (self.size.1 - bottom_slop)
        }
    }
}

// Helper function for choosing between normal and shifted action
fn s<'a>(mods: u32, normal: &'a str, shifted: &'a str) -> &'a str {
    if (mods & M_SHIFT) != 0 { shifted } else { normal }
}

impl Resources {
    fn create_text_layout(&self, factory: &directwrite::Factory, text: &str) -> TextLayout {
        let params = text_layout::ParamBuilder::new()
            .text(text)
            .font(self.text_format.clone())
            .width(1e6)
            .height(1e6)
            .build().unwrap();
        factory.create(params).unwrap()
    }
}
