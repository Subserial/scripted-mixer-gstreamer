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

use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::fs;

use gstreamer::prelude::{Cast, ElementExt, GstBinExt};
use gtk::glib::ObjectExt;

use crate::{catch_bail, catch_bail_annotate, none_bail};
use crate::error::{ParseError, ParseResult};
use crate::script::action::set_property;

mod action;

pub enum ParsedSetting {
    Int(i32),
    Float(f64),
    String(String),
}

pub struct Template {
    pipeline: String,
    arg_count: usize,
    settings: HashMap<String, Vec<(String, String, String)>>,
}

impl Template {
    pub fn parse_command<'a, I>(args: &[&str], str_iter: &mut I) -> ParseResult<(String, Template)>
        where I: Iterator<Item = &'a str>,
    {
        let name = args[0].to_string();
        let arg_count = catch_bail!(args[1].parse::<usize>(), "could not parse parameter count");
        let pipeline = args[2..].join(" ");
        let mut h = HashMap::new();
        loop {
            let s = str_iter.next();
            if s.is_none() {
                return Err(ParseError::report("Iterator ended before constructing custom pipeline"));
            }
            let s = String::from(s.unwrap());
            if s.starts_with("war") {
                return Ok((name, Template {
                    pipeline,
                    arg_count,
                    settings: h,
                }));
            }
            let vals: Vec<&str> = s.split_whitespace().collect();
            if vals.len() != 4 {
                return Err(ParseError::report("Incorrect number of arguments in custom pipeline instruction"));
            }
            let prop_instructions = (
                vals[1].to_string(),
                vals[2].to_string(),
                vals[3].to_string(),
            );
            if h.contains_key(vals[0]) {
                h.get_mut(vals[0]).unwrap().push(prop_instructions);
            } else {
                let mut params = Vec::new();
                params.push(prop_instructions);
                h.insert(vals[0].to_string(), params);
            }
        }
    }

    pub fn generate(&self, name: String, args: &[&str]) -> ParseResult<(gstreamer::Pipeline, HashMap<String, ParsedSetting>)> {
        if args.len() != self.arg_count {
            return Err(ParseError::report("Incorrect number of arguments"));
        };
        let rendered = catch_bail!(gstreamer::parse_launch(self.pipeline.as_str()), "Could not render pipeline");
        catch_bail!(set_property(rendered.clone(), "name".to_string(), "string".to_string(), name), "Could not name pipeline");
        let pipeline = catch_bail!(rendered.clone().dynamic_cast::<gstreamer::Pipeline>(), "Could not cast pipeline to pipeline");
        let mut external = HashMap::new();

        for (key, value) in self.settings.iter() {
            for (prop, type_, val) in value.iter() {
                let actual_val = if val.starts_with("$") {
                    let index = catch_bail!(val[1..].parse::<usize>(), "Could not parse argument index");
                    if index > args.len() {
                        return Err(ParseError::report("Argument index greater than number of arguments"))
                    }
                    args[index - 1]
                } else {
                    val.as_str()
                };
                if key == "raw" {
                    let ext = match type_.as_str() {
                        "int" => {
                            ParsedSetting::Int(catch_bail!(actual_val.parse(), "Failed to parse setting as int"))
                        },
                        "float" => {
                            ParsedSetting::Float(catch_bail!(actual_val.parse(), "Failed to parse setting as float"))
                        },
                        "string" => {
                            ParsedSetting::String(actual_val.to_string())
                        },
                        _ => return Err(ParseError::report_string(format!("Unknown type: {}", type_))),
                    };
                    external.insert(prop.clone(), ext);
                } else {
                    let elem_prop = none_bail!(pipeline.by_name(key), format!("Could not obtain element {}", key));
                    catch_bail_annotate!(action::set_property(elem_prop, prop.clone(), type_.clone(), actual_val.to_string()), "set_property");
                }
            }
        };

        Ok((pipeline, external))
    }
}

static MP3INPUT_PATTERN: &str =
    "raw mp3input 1 filesrc name=src ! decodebin \
    ! audioconvert ! audioresample ! proxysink name=audio_out\n\
    src location string $1\n\
    war";

static MP4INPUT_PATTERN: &str =
    "raw mp4input 1 filesrc name=src ! decodebin name=demux \
    demux. ! videoconvert ! proxysink name=video_out \
    demux. ! audioconvert ! audioresample ! proxysink name=audio_out\n\
    src location string $1\n\
    war";

static ALSAOUTPUT_PATTERN: &str =
    "raw aoutput 0 proxysrc name=audio_in ! alsasink name=sink\n\
    war";

static XOUTPUT_PATTERN: &str =
    "raw xoutput 4 proxysrc name=video_in ! xvimagesink name=sink\n\
    raw gtktag string window\n\
    raw x int $1\n\
    raw y int $2\n\
    raw width int $3\n\
    raw height int $4\n\
    war";

pub struct Pattern {
    pub blocks: HashMap<String, Template>,
    pub pipes: HashMap<String, (gstreamer::Pipeline, HashMap<String, ParsedSetting>)>,
    pub listen: Option<crossbeam_channel::Receiver<String>>,
    pub time_events: HashMap<(String, u64), Vec<Box<dyn action::EventAction>>>,
    pub pre_events: Vec<Box<dyn action::EventAction>>,
}

impl Pattern {
    fn default() -> Pattern {
        let mut pipelines = HashMap::new();
        let premade = vec![MP3INPUT_PATTERN, MP4INPUT_PATTERN, XOUTPUT_PATTERN, ALSAOUTPUT_PATTERN];
        for pre in premade {
            let mut lines = pre.split('\n');
            let args = lines.next().unwrap().split_whitespace().collect::<Vec<&str>>();
            let (name, pipeline) = Template::parse_command(&args[1..], &mut lines).unwrap();
            pipelines.insert(name, pipeline);
        }
        Pattern {
            blocks: pipelines,
            pipes: HashMap::new(),
            listen: None,
            time_events: HashMap::new(),
            pre_events: Vec::new(),
        }
    }

    pub fn parse_pattern(path: String) -> ParseResult<Pattern> {
        let data = match fs::read_to_string(path) {
            Ok(d) => d,
            Err(err) => return Err(ParseError::report_string(format!("Could not read file: {:?}", err))),
        };
        let lines = data.split("\n");
        let mut commands = Vec::new();
        let mut pattern = Pattern::default();
        let (tx, rx) = crossbeam_channel::unbounded();
        pattern.listen = Some(rx);
        // clear comments
        for line in lines {
            if !line.starts_with("//") && line.len() != 0 {
                commands.push(line);
            }
        };

        let mut cmd_iter = commands.into_iter();
        loop {
            let line = cmd_iter.next();
            if line.is_none() {
                break
            }
            let line = line.unwrap();
            let args = line.split_whitespace().collect::<Vec<&str>>();
            match args[0] {
                "raw" => {
                    let (name, pipeline) = match Template::parse_command(&args[1..], &mut cmd_iter) {
                        Ok(rst) => rst,
                        Err(err) => return Err(err.annotate("parse_command")),
                    };
                    pattern.blocks.insert(name, pipeline);
                },
                "new" => {
                    let key = args[1];
                    let name = args[2].to_string();
                    let pipeline_args = &args[3..];
                    let block = pattern.blocks.get(key).unwrap();
                    match block.generate(name.clone(), pipeline_args) {
                        Ok((elem, settings)) => {
                            pattern.pipes.insert(name, (elem, settings));
                        },
                        Err(err) => return Err(err.annotate("block.generate")),
                    }
                },
                "plug" => {
                    let (pipe_a, _) = none_bail!(pattern.pipes.get(args[2]), format!("No pipe found: {}", args[2]));
                    let (pipe_b, _) = none_bail!(pattern.pipes.get(args[4]), format!("No pipe found: {}", args[4]));
                    let elem_a = none_bail!(pipe_a.clone().by_name(args[1]), format!("No element found: {}", args[1]));
                    let elem_b = none_bail!(pipe_b.clone().by_name(args[3]), format!("No element found: {}", args[3]));
                    elem_b.set_property("proxysink", elem_a);
                    println!("{}->{} ==> {}->{}", args[2], args[1], args[4], args[3]);
                },
                "on" => {
                    let type_ = args[1];
                    match type_ {
                        "callback" => {
                            let (pipe, _) = none_bail!(pattern.pipes.get(args[2]), format!("No element found: {}", args[2]));
                            let event = args[3];
                            match event {
                                "end" => {
                                    let bus = pipe.bus().unwrap();
                                    let events = catch_bail_annotate!(action::parse_leading_event(&mut pattern, tx.clone(), &args[4..], &mut cmd_iter), "callback end");
                                    bus.add_signal_watch();
                                    bus.connect("message::eos", true,
                                                move |_| {
                                                    for event in &events {
                                                        event.exec();
                                                    }
                                                    None
                                                }
                                    );
                                }
                                e => return Err(ParseError::report_string(format!("Unknown callback: {}", e)))
                            }
                        },
                        "pre" => {
                            let events = catch_bail_annotate!(action::parse_leading_event(&mut pattern, tx.clone(), &args[2..], &mut cmd_iter), "pre, parse");
                            for event in events {
                                pattern.pre_events.push(event)
                            }
                        },
                        "progress" => {
                            none_bail!(pattern.pipes.get(args[2]), format!("No element found: {}", args[2]));
                            let p_name = args[2];
                            let time = catch_bail!(args[3].parse::<f64>(), "Failed to parse time");
                            let nanotime = (time * 1000000000.0) as u64;
                            let events = catch_bail_annotate!(action::parse_leading_event(&mut pattern, tx.clone(), &args[4..], &mut cmd_iter), "progress, parse");
                            match pattern.time_events.borrow_mut().get_mut(&(p_name.to_string(), nanotime)) {
                                Some(v) => {
                                    for event in events.into_iter() {
                                        v.push(event)
                                    }
                                },
                                None => {
                                    pattern.time_events.borrow_mut().insert((p_name.to_string(), nanotime), events);
                                },
                            }
                        }
                        t => return Err(ParseError::report_string(format!("Unknown event type: {}", t)))
                    }
                },
                cmd => return Err(ParseError::report_string(format!("Unknown command: {}", cmd))),
            };
        }
        Ok(pattern)
    }
}
