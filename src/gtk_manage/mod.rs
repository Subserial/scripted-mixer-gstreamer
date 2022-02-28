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

use std::borrow::Borrow;
use std::cell::{RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;
use gstreamer::prelude::{ElementExtManual, GstBinExt};
use gstreamer_video::prelude::VideoOverlayExtManual;
use gtk::prelude::{Cast, ApplicationCommandLineExt, ApplicationExt};

use gtk::traits::*;
use crate::none_bail;

use crate::script::{ParsedSetting, Pattern};

pub mod window;

struct MovingPart {
    pipeline: gstreamer::Pipeline,
    window: String,
    start: i64,
    end: i64,
    start_x: i32,
    end_x: i32,
    start_y: i32,
    end_y: i32,
    path_x: String,
    path_y: String,
}

pub fn run_app(app: &gtk::Application, args: &gtk::gio::ApplicationCommandLine) -> i32 {
    app.hold();
    let args = args.clone().arguments();
    if args.len() < 2 {
        println!("Usage: {} PATTERN_PATH", args[0].clone().into_string().unwrap());
    }
    let path = match args[1].clone().into_string() {
        Ok(p) => p,
        Err(_) => {
            println!("Couldn't parse pattern path");
            app.release();
            return 1;
        }
    };

    let p = Pattern::parse_pattern(path);
    let pattern = match p {
        Ok(p) => p,
        Err(err) => {
            println!("Error! {:?}", err);
            app.release();
            return 1;
        }
    };

    let mut windows: HashMap<String, (Rc<RefCell<gtk::Window>>, u32)> = HashMap::new();

    for (name, elem) in &pattern.pipes {
        let (elem, tags) = elem;
        let mut needs_window = false;
        for (tag, setting) in tags {
            if tag == "gtktag" {
                match setting {
                    ParsedSetting::String(s) => {
                        if s == "window" {
                            needs_window = true;
                            break;
                        }
                    },
                    _ => ()
                }
            }
        }
        if needs_window {
            let (mut x, mut y, mut width, mut height) = (0, 0, 0, 0);
            for (tag, setting) in tags {
                match tag.as_str() {
                    "x" => {
                        match setting {
                            ParsedSetting::Int(i) => x = *i,
                            _ => println!("bad window parameter"),
                        }
                    }
                    "y" => {
                        match setting {
                            ParsedSetting::Int(i) => y = *i,
                            _ => println!("bad window parameter"),
                        }
                    }
                    "width" => {
                        match setting {
                            ParsedSetting::Int(i) => width = *i,
                            _ => println!("bad window parameter"),
                        }
                    }
                    "height" => {
                        match setting {
                            ParsedSetting::Int(i) => height = *i,
                            _ => println!("bad window parameter"),
                        }
                    }
                    _ => ()
                }
            }
            let (window, xid) = window::create_gtk_window(x, y, width, height);
            window.hide();

            let pipeline = elem.clone().dynamic_cast::<gstreamer::Pipeline>().unwrap();
            let sink_elem = pipeline.by_name("sink").unwrap();
            let sink = sink_elem.dynamic_cast::<gstreamer_video::VideoOverlay>().unwrap();
            unsafe {
                sink.set_window_handle(xid as usize);
            }
            windows.insert(name.clone(), (Rc::new(RefCell::new(window)), xid));
        }
    }

    for event in &pattern.pre_events {
        event.exec();
    }

    let mut moving_parts: Vec<MovingPart> = Vec::new();
    let recv = pattern.listen.as_ref().unwrap().clone();
    let mut nano_map: HashSet<(String, u64)> = HashSet::new();
    gtk::glib::timeout_add_local(Duration::from_millis(10), move || {
        while let Ok(msg) = recv.try_recv() {
            println!("Received message: {}", msg.clone());
            match handle_message(&pattern, &windows, msg, &mut moving_parts) {
                Ok(b) => if !b {return gtk::glib::Continue(false)},
                Err(e) => println!("Error from message: {}", e)
            }
        }

        for ((name, nanos), events) in &pattern.time_events {
            if nano_map.contains(&(name.clone(), nanos.clone())) {
                continue
            }
            let p = match pattern.pipes.get(name) {
                Some((p, _)) => p,
                None => panic!("Could not find node {}", name),
            };
            let d = p.clone().query_position_generic(gstreamer::Format::Time);
            if let Some(d) = d {
                let current_nanos = d.value();
                if (current_nanos) > (nanos.clone() as i64) {
                    nano_map.insert((name.clone(), nanos.clone()));
                    for event in events {
                        event.exec();
                    }
                }
            }
        }

        let mut remove_parts: Vec<usize> = Vec::new();
        for (i, part) in moving_parts.iter().enumerate() {
            let time = match part.pipeline.query_position_generic(gstreamer::Format::Time) {
                Some(v) => v.value(),
                None => continue,
            };

            let (win, _) = windows.get(&part.window).unwrap();
            let wrc: Rc<RefCell<gtk::Window>> = win.clone();
            let window: &RefCell<gtk::Window> = wrc.borrow();
            let wc = window.borrow_mut();
            if time < part.start {
                continue;
            }
            if time > part.end {
                wc.move_(part.end_x, part.end_y);
                remove_parts.push(i);
                continue;
            }

            let time_frac = (time - part.start) as f64 / (part.end - part.start) as f64;
            let x_diff = part.end_x - part.start_x;
            let y_diff = part.end_y - part.start_y;
            let x_abs = part.start_x + match part.path_x.as_str() {
                "mcos" => x_diff as f64 * (1.0 - f64::to_radians(time_frac * 90.0).cos()),
                _ => x_diff as f64,
            } as i32;
            let y_abs = part.start_y + match part.path_y.as_str() {
                "mcos" => y_diff as f64 * (1.0 - f64::to_radians(time_frac * 90.0).cos()),
                _ => y_diff as f64,
            } as i32;
            wc.move_(x_abs, y_abs);
        }

        let mut offset = 0;
        for i in remove_parts {
            moving_parts.remove(i - offset);
            offset = offset + 1;
        }

        gtk::glib::Continue(true)
    });
    0
}

fn handle_message(pattern: &Pattern, windows: &HashMap<String, (Rc<RefCell<gtk::Window>>, u32)>, msg: String, parts: &mut Vec<MovingPart>) -> Result<bool, String> {
    if msg == " terminate" {
        std::process::exit(0);
    }
    if msg == "pre" {
        for event in &pattern.pre_events {
            event.exec();
        }
        return Ok(true);
    }
    let args = msg.split_whitespace().collect::<Vec<&str>>();
    let mut args_iter = args.into_iter();
    let window = args_iter.next().unwrap();
    let action = args_iter.next().unwrap();
    let args = args_iter.collect::<Vec<&str>>();
    match action {
        "show" => {
            let (win, _) = none_bail!(windows.get(window), format!("Unknown window: {}", action));
            let wc = win.clone();
            let wrc: &RefCell<gtk::Window>= wc.borrow();
            let window: RefMut<gtk::Window> = wrc.borrow_mut();
            window.show_all();
        }
        "move" => {
            let key = args[0];
            let (pipeline, _) = none_bail!(pattern.pipes.get(key), format!("Pipeline not found: {}", key));
            let pipeline = pipeline.clone().dynamic_cast::<gstreamer::Pipeline>().unwrap();
            let start_time = (args[1].parse::<f32>().unwrap() * 1_000_000_000.0) as i64;
            let start_x = args[2].parse::<i32>().unwrap();
            let start_y = args[3].parse::<i32>().unwrap();
            let final_time = (args[4].parse::<f32>().unwrap() * 1_000_000_000.0) as i64;
            let final_x = args[5].parse::<i32>().unwrap();
            let final_y = args[6].parse::<i32>().unwrap();
            let mode_x = args[7];
            let mode_y = args[8];
            let part = MovingPart {
                pipeline,
                window: window.to_string(),
                start: start_time,
                end: final_time,
                start_x,
                start_y,
                end_x: final_x,
                end_y: final_y,
                path_x: mode_x.to_string(),
                path_y: mode_y.to_string(),
            };
            parts.push(part);
        }
        _ => return Err(format!("Unknown action: {}", action))
    }
    return Ok(true);
}
