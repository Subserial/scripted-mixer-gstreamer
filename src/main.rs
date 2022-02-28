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

use gtk::prelude::{ApplicationExt, ApplicationExtManual};

mod script;
mod gtk_manage;
mod error;

fn main() -> Result<(), i32> {
    if gstreamer::init().is_err() {
        println!("Could not initialize gstreamer.");
        std::process::exit(1);
    }
    if gtk::init().is_err() {
        println!("Could not initialize gtk.");
        std::process::exit(1);
    }

    let app = gtk::Application::builder()
        .application_id("dev.subsy.live-mix")
        .build();
    app.set_flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE);

    app.connect_command_line(gtk_manage::run_app);
    let ret = app.run();
    if ret == 0 {
        Ok(())
    } else {
        Err(ret)
    }
}
