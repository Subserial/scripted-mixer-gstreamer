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

use gstreamer::ClockTime;
use gstreamer::prelude::*;
use crate::catch_bail;

use crate::error::{ParseError, ParseResult};
use crate::script::Pattern;

pub trait EventAction: Send + Sync {
    fn exec(&self);
}

pub struct WindowAction {
    window: String,
    action: String,
    settings: Vec<String>,
    chan: crossbeam_channel::Sender<String>,
}

unsafe impl Send for WindowAction {}
unsafe impl Sync for WindowAction {}

impl EventAction for WindowAction {
    fn exec(&self) {
        let mut s = self.window.clone();
        s.push_str(" ");
        s.push_str(self.action.as_str());
        for setting in self.settings.iter() {
            s.push_str(" ");
            s.push_str(setting.as_str());
        }
        let sent = self.chan.send(s);
        if let Err(_) = sent {
            panic!("Couldn't send window action over channel")
        }
    }
}

pub struct PlayAction {
    pipeline: gstreamer::Pipeline,
    action: String,
}

unsafe impl Send for PlayAction {}
unsafe impl Sync for PlayAction {}

impl EventAction for PlayAction {
    fn exec(&self) {
        let pipeline = self.pipeline.clone().dynamic_cast::<gstreamer::Pipeline>().unwrap();
        let action = match self.action.as_str() {
            "start" => {
                gstreamer::State::Playing
            },
            "pause" => {
                gstreamer::State::Paused
            },
            "ready" => {
                gstreamer::State::Ready
            },
            "null" => {
                gstreamer::State::Null
            },
            a => panic!("Unknown pipeline state: {}", a)
        };
        match pipeline.set_state(action) {
            Ok(_) => println!("Element {} set to {}", self.pipeline.name(), self.action),
            Err(err) => println!("PlayAction state change error: {:?}", err)
        }
    }
}

pub struct SeekAction {
    pipeline: gstreamer::Pipeline,
    rate: f64,
    time: f64,
}

unsafe impl Send for SeekAction {}
unsafe impl Sync for SeekAction {}

impl EventAction for SeekAction {
    fn exec(&self) {
        let result = self.pipeline.seek(
            self.rate, gstreamer::SeekFlags::FLUSH,
            gstreamer::SeekType::Set, ClockTime::from_nseconds((self.time * 1000000.0) as u64),
            gstreamer::SeekType::None, ClockTime::from_nseconds(0),
        );
        if let Err(_) = result {
            println!("Seek event was not handled for pipeline {}", self.pipeline.name())
        }
    }
}

pub struct SetPropAction {
    element: gstreamer::Element,
    prop: String,
    type_: String,
    setting: String,
}

unsafe impl Send for SetPropAction {}
unsafe impl Sync for SetPropAction {}

impl EventAction for SetPropAction {
    fn exec(&self) {
        let status = set_property(self.element.clone(), self.prop.clone(), self.type_.clone(), self.setting.clone());
        if let Err(err) = status {
            panic!("{}",err.annotate("set_property"));
        }
    }
}

pub fn set_property(element: gstreamer::Element, prop: String, type_: String, value: String) -> ParseResult<()> {
    match type_.as_str() {
        "int" => {
            let val = catch_bail!(value.parse::<i32>(), format!("Unable to parse {} as {}", value, type_));
            element.set_property(prop.as_str(), val);
            Ok(())
        },
        "float" => {
            let val = catch_bail!(value.parse::<f64>(), format!("Unable to parse {} as {}", value, type_));
            element.set_property(prop.as_str(), val);
            Ok(())
        }
        "string" => {
            element.set_property(prop.as_str(), value);
            Ok(())
        },
        "GstOrientation" => {
            let direction = match value.as_str() {
                "0" => gstreamer_video::VideoOrientationMethod::Identity,
                "1" => gstreamer_video::VideoOrientationMethod::_90r,
                "2" => gstreamer_video::VideoOrientationMethod::_180,
                "3" => gstreamer_video::VideoOrientationMethod::_90l,
                _ => return Err(ParseError::report_string(format!("Unknown GstOrientation index: {}", value)))
            };
            element.set_property(prop.as_str(), direction);
            Ok(())
        }
        _ => return Err(ParseError::report_string(format!("Unknown parameter type: {}", type_))),
    }
}

pub fn parse_leading_event<'a, I>(pattern: &mut Pattern, chan: crossbeam_channel::Sender<String>, args: &[&str], cmd_iter: &mut I) -> ParseResult<Vec<Box<dyn EventAction>>>
    where I: Iterator<Item = &'a str>, {
    let mut actions = Vec::new();
    match args[0] {
        "terminate" => {
            actions.push(
                Box::new(WindowAction {
                    window: "".to_string(),
                    action: "terminate".to_string(),
                    settings: vec![],
                    chan
                }) as Box<dyn EventAction>
            );
        },
        "act" => {
            match parse_single_event(&pattern, chan.clone(), args) {
                Ok(t) => actions.push(t),
                Err(e) => return Err(e.annotate("act")),
            };
        },
        "wrap" => {
            loop {
                let line = cmd_iter.next().unwrap();
                let args: Vec<&str> = line.split_whitespace().collect();
                if args[0] == "parw" {
                    break;
                }

                match parse_single_event(&pattern, chan.clone(), &args[..]) {
                    Ok(t) => actions.push(t),
                    Err(e) => return Err(e.annotate("wrap")),
                };
            };
        },
        a => return Err(ParseError::report_string(format!("Unknown condition: {}", a))),
    }

    Ok(actions)
}

fn parse_single_event(pattern: &Pattern, chan: crossbeam_channel::Sender<String>, args: &[&str]) -> ParseResult<Box<dyn EventAction>> {
    let result = match args[0] {
        "act" => {
            let elem = match pattern.pipes.get(args[1]) {
                Some(e) => e.0.clone(),
                None => return Err(ParseError::report_string(format!("Unknown pipeline: {}", args[1])))
            };
            let pipeline = elem.clone().dynamic_cast::<gstreamer::Pipeline>().unwrap();
            match args[2] {
                "prop" => {
                    let mod_elem = match pipeline.by_name(args[3]) {
                        Some(e) => e,
                        None => return Err(ParseError::report_string(format!("Unknown element: {}", args[3]))),
                    };
                    Box::new(SetPropAction {
                        element: mod_elem,
                        prop: args[4].to_string(),
                        type_:args[5].to_string(),
                        setting: args[6..].join(" "),
                    }) as Box<dyn EventAction>
                },
                "play" => {
                    Box::new(PlayAction {
                        pipeline: elem,
                        action: args[3].to_string(),
                    }) as Box<dyn EventAction>
                },
                "seek" => {
                    let time = match args[3].parse::<f64>() {
                        Ok(f) => f,
                        Err(_) => return Err(ParseError::report_string(format!("Could not parse as float: {}", args[3]))),
                    };
                    let rate = match args[4].parse::<f64>() {
                        Ok(f) => f,
                        Err(_) => return Err(ParseError::report_string(format!("Could not parse as float: {}", args[4]))),
                    };
                    Box::new(SeekAction {
                        pipeline: elem,
                        rate,
                        time,
                    }) as Box<dyn EventAction>
                },
                "window" => {
                    Box::new(WindowAction {
                        window: args[1].to_string(),
                        action: args[3].to_string(),
                        settings: args[4..].iter().map(|&s| s.into()).collect::<Vec<String>>(),
                        chan: chan.clone(),
                    }) as Box<dyn EventAction>
                },
                a => return Err(ParseError::report_string(format!("Unkown event type: {}", a))),
            }
        },
        a => return Err(ParseError::report_string(format!("Unknown event header: {}", a))),
    };
    Ok(result)
}
