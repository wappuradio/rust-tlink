#[macro_use]
extern crate gstreamer as gst;
use gst::prelude::*;
extern crate glib;


use std::env;
use std::error::Error as StdError;
use std::time;
use std::thread;

#[path = "../common.rs"]
mod common;

extern crate failure;
use failure::Error;

#[macro_use]
extern crate failure_derive;

#[derive(Debug, Fail)]
#[fail(display = "Missing element {}", _0)]
struct MissingElement(&'static str);

#[derive(Debug, Fail)]
#[fail(display = "No such pad {} in {}", _0, _1)]
struct NoSuchPad(&'static str, String);

#[derive(Debug, Fail)]
#[fail(display = "Unknown payload type {}", _0)]
struct UnknownPT(u32);

#[derive(Debug, Fail)]
#[fail(display = "Usage: {} PORT LATENCY SIZE-TIME(ms)", _0)]
struct UsageError(String);

#[derive(Debug, Fail)]
#[fail(display = "Received error from {}: {} (debug: {:?})", src, error, debug)]
struct ErrorMessage {
    src: String,
    error: String,
    debug: Option<String>,
    #[cause]
    cause: glib::Error,
}

fn make_element<'a, P: Into<Option<&'a str>>>(
    factory_name: &'static str,
    element_name: P,
) -> Result<gst::Element, Error> {
    match gst::ElementFactory::make(factory_name, element_name.into()) {
        Some(elem) => Ok(elem),
        None => Err(Error::from(MissingElement(factory_name))),
    }
}

fn get_static_pad(element: &gst::Element, pad_name: &'static str) -> Result<gst::Pad, Error> {
    match element.get_static_pad(pad_name) {
        Some(pad) => Ok(pad),
        None => {
            let element_name = element.get_name();
            Err(Error::from(NoSuchPad(pad_name, element_name)))
        }
    }
}

fn get_request_pad(element: &gst::Element, pad_name: &'static str) -> Result<gst::Pad, Error> {
    match element.get_request_pad(pad_name) {
        Some(pad) => Ok(pad),
        None => {
            let element_name = element.get_name();
            Err(Error::from(NoSuchPad(pad_name, element_name)))
        }
    }
}

fn connect_rtpbin_srcpad(src_pad: &gst::Pad, sink: &gst::Element) -> Result<(), Error> {
    let name = src_pad.get_name();
    let split_name = name.split("_");
    let split_name = split_name.collect::<Vec<&str>>();
    let pt = split_name[5].parse::<u32>()?;
    match pt {
        96 => {
            let sinkpad = get_static_pad(sink, "sink")?;
            src_pad.link(&sinkpad).into_result()?;
            Ok(())
        }
        _ => Err(Error::from(UnknownPT(pt))),
    }
}

fn make_fec_decoder(rtpbin: &gst::Element, sess_id: u32) -> Result<gst::Element, Error> {
    let fecdec = make_element("rtpulpfecdec", "fecdec")?;
    let internal_storage = rtpbin
        .emit("get-internal-storage", &[&sess_id.to_value()])
        .unwrap()
        .unwrap();

    fecdec.set_property("storage", &internal_storage.to_value())?;
    fecdec.set_property("pt", &100u32.to_value())?;
    println!("Making fecdec");
    let recovered = fecdec.get_property("recovered");
    let unrecovered = fecdec.get_property("unrecovered");
    println!("{:?}",recovered);
    println!("{:?}", unrecovered);
    Ok(fecdec)
}

fn example_main() -> Result<(), Error> {
    gst::init()?;

    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        return Err(Error::from(UsageError(args[0].clone())));
    }

   // let address = args[1].parse::<String>()?;
    let port = args[1].parse::<i32>()?;
    let latency = args[2].parse::<u32>()?;
    let size_time_ms = args[3].parse::<u64>()?;

    let pipeline = gst::Pipeline::new(None);
    let udpsrc = make_element("udpsrc", None)?;
    let rtpbin = make_element("rtpbin", None)?;
    let rtpopusdepay = make_element("rtpopusdepay", "depay")?;
    let queue1 = make_element("queue", None)?;
    let opusdec = make_element("opusdec", None)?;
    let queue2 = make_element("queue", None)?;
    let audioconvert = make_element("audioconvert", None)?;
    let jackaudiosink = make_element("jackaudiosink", None)?;

    /*
    let netsim = make_element("netsim", None)?;
    let depay = make_element("rtpvp8depay", None)?;
    let dec = make_element("vp8dec", None)?;
    let conv = make_element("videoconvert", None)?;
    let scale = make_element("videoscale", None)?;
    let filter = make_element("capsfilter", None)?;
	*/
    pipeline.add_many(&[&udpsrc, &rtpbin, &rtpopusdepay, &queue1, &opusdec, &queue2, &audioconvert, &jackaudiosink])?;
    // TODO: Check what actually need to be linked
    gst::Element::link_many(&[&rtpopusdepay, &queue1, &opusdec, &queue2, &audioconvert, &jackaudiosink])?;


    rtpbin.connect("new-storage", false, move |values| {
        let storage = values[1].get::<gst::Element>().expect("Invalid argument");
        let size_time_ns  = &size_time_ms * 1000000u64;
        storage
            .set_property("size-time", &size_time_ns.to_value())
            .unwrap();

        None
    })?;

    rtpbin.connect("request-pt-map", false, |values| {
        let pt = values[2].get::<u32>().expect("Invalid argument");
        match pt {
            100 => Some(
                gst::Caps::new_simple(
                    "application/x-rtp",
                    &[
                        ("media", &"audio"),
                        ("clock-rate", &48000i32),
                        ("is-fec", &true),
                    ],
                ).to_value(),
            ),
            96 => Some(
                gst::Caps::new_simple(
                    "application/x-rtp",
                    &[
                        ("media", &"audio"),
                        ("clock-rate", &48000i32),
                        ("encoding-name", &"OPUS"),
                    ]
                ).to_value(),
            ),
            _ => None,
        }
    })?;

   
    //udpsrc.link(&rtpopusdepay);

    //udpsrc.link(&rtpbin); 
    let srcpad = get_static_pad(&udpsrc, "src")?;
    let sinkpad = get_request_pad(&rtpbin, "recv_rtp_sink_0")?;
    srcpad.link(&sinkpad).into_result()?;
    
   /* let srcpad2 = get_request_pad(&rtpbin, "send_rtp_src_0")?;
    let sinkpad2 = get_static_pad(&rtpopusdepay, "sink")?;
    srcpad2.link(&sinkpad2).into_result()?;*/
   // rtpbin.link(&queue)?;
    //This is probably unnecessary for us, maybe?
    //Lets look into pads at some point, shall we?
    /*
    let srcpad = get_static_pad(&rtpbin, None)?;
    let sinkpad = get_request_pad(&rtpbin, "recv_rtp_sink_0")?;
    srcpad.link(&sinkpad).into_result()?;
    */
    // How this works depend on the implementation of the library.
    let depay_clone = rtpopusdepay.clone();
    //rtpbin.link(&rtpopusdepay)?;
    rtpbin.connect_pad_added(move |rtpbin, src_pad| {
        rtpbin.unlink(&depay_clone);
     
        match connect_rtpbin_srcpad(&src_pad, &depay_clone) {
            Ok(_) => (),
            Err(err) => {
                gst_element_error!(
                    rtpbin,
                    gst::LibraryError::Failed,
                    ("Failed to link srcpad"),
                    ["{}", err]
                );
                ()
            }
        }
        rtpbin.connect("request-fec-decoder", false, |values| {
        let rtpbin = values[0].get::<gst::Element>().expect("Invalid argument");
        let sess_id = values[1].get::<u32>().expect("Invalid argument");
        println!("Requesting fecdec");
        match make_fec_decoder(&rtpbin, sess_id) {
            Ok(elem) => Some(elem.to_value()),
            Err(err) => {
                gst_element_error!(
                    rtpbin,
                    gst::LibraryError::Failed,
                    ("Failed to make FEC decoder"),
                    ["{}", err]
                );
                None
            }
        }
        });
    });

    let rtp_caps = gst::Caps::new_simple("application/x-rtp", &[("clock-rate", &48000i32)]);
    
    udpsrc.set_property("port", &port.to_value())?;
    udpsrc.set_property("caps", &rtp_caps.to_value())?;
    rtpbin.set_property("do-lost", &true.to_value())?;
    rtpbin.set_property("latency", &latency.to_value())?;
    opusdec.set_property("plc", &true.to_value())?;
    jackaudiosink.set_property("buffer-time", &100000i64.to_value())?;
    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    let ret = pipeline.set_state(gst::State::Playing);
    assert_ne!(ret, gst::StateChangeReturn::Failure);

    let pipelineclone = pipeline.clone();
    let stats_thread = thread::spawn(move || {
        loop {
        match pipelineclone.get_by_name("fecdec") {
            Some(fecdec) => {
                               //  println!("FecDec {:?}", fecdec);
                                let recovered = fecdec.get_property("recovered");
                                let unrecovered = fecdec.get_property("unrecovered");
                                println!("Recovered packets: {:?}", recovered);
                                println!("Unrecovered packets: {:?}", unrecovered);

                             },
            None => {
                        println!("Was not Some");
                        },
        }
        match pipelineclone.get_by_name("rtpjitterbuffer0") {
            Some(session) => {
                let stats = session.get_property("stats").unwrap();
                println!("{:?}", stats);
               // let received = gstreamer_sys::gst_structure_get_value(&stats, "packets-received");

            }, 
            None => {
                println!("Did not get jitterbuffer");
            }
        }
         match pipelineclone.get_by_name("depay") {
            Some(depay) => {
                let state = depay.get_state(gst::CLOCK_TIME_NONE);
                println!("{:?}", state);
               // let received = gstreamer_sys::gst_structure_get_value(&stats, "packets-received");

            }, 
            None => {
                println!("Did not get opusdepay");
            }
        }
        gst::debug_bin_to_dot_file_with_ts(
                        &pipelineclone,
                        gst::DebugGraphDetails::all(),
                        "client-playing-thread",
                    );
        thread::sleep(time::Duration::from_millis(500));
        }
    });





   // gst::debug_set_active(true);
    //tässä tod näk käy niin, että suoritus "jää jumiin" timed_pop:iin
    //Pistä threadiin, jos haluat välttä
    while let Some(msg) = bus.timed_pop(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(..) => {
                println!("Stream ended");
            },
            MessageView::Error(err) => {
                return Err(ErrorMessage {
                    src: msg.get_src()
                        .map(|s| s.get_path_string())
                        .unwrap_or(String::from("None")),
                    error: err.get_error().description().into(),
                    debug: err.get_debug(),
                    cause: err.get_error(),
                }.into());
            }
            MessageView::StateChanged(s) => match msg.get_src() {
                Some(element) => if element == pipeline && s.get_current() == gst::State::Playing {
                    eprintln!("PLAYING");
                    gst::debug_bin_to_dot_file(
                        &pipeline,
                        gst::DebugGraphDetails::all(),
                        "client-playing",
                    );
                },
                None => (),
            },
            _ => (),
        }
    }
    let ret = pipeline.set_state(gst::State::Null);
    assert_ne!(ret, gst::StateChangeReturn::Failure);
    let _ = stats_thread.join();
    Ok(())
}

fn main() {
    match common::run(example_main) {
        Ok(r) => r,
        Err(e) => eprintln!("Error! {}", e),
    }
}
