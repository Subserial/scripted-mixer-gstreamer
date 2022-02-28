/*
 * Copyright 2022 Google LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     https://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
*/

use gtk::traits::*;

extern {
    fn gdk_x11_window_get_xid(window: gtk::gdk::Window) -> u32;
}

pub fn create_gtk_window(x: i32, y: i32, width: i32, height: i32) -> (gtk::Window, u32) {
    let wnd = gtk::Window::new(gtk::WindowType::Toplevel);
    wnd.set_border_width(0);
    wnd.set_decorated(false);
    wnd.move_(x, y);
    wnd.set_default_size(width, height);
    let dar = gtk::DrawingArea::new();
    wnd.add(&dar);
    wnd.show_all();

    let ndw = dar.window().unwrap();
    let xid = unsafe {gdk_x11_window_get_xid(ndw)};
    println!("Obtained window with xid {}", xid);
    (wnd, xid)
}
