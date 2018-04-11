#[macro_use]
extern crate gstreamer as gst;
use gst::prelude::*;

extern crate glib;

use std::error::Error as StdError;

#[path = "../common.rs"]
mod common;

use std::env;

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
#[fail(display = "Usage: {} ADDRESS PORT OPUS_BITRATE OPUS_FRAME_SIZE PERCENTAGE PERCENTAGE_IMPORTANT WAVE FREQ", _0)]
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
/*
#[derive(Debug)]
enum GstAudioTestSrcWave {
   GST_AUDIO_TEST_SRC_WAVE_SINE,
   GST_AUDIO_TEST_SRC_WAVE_SQUARE,
   GST_AUDIO_TEST_SRC_WAVE_SAW,
   GST_AUDIO_TEST_SRC_WAVE_TRIANGLE,
   GST_AUDIO_TEST_SRC_WAVE_SILENCE,
   GST_AUDIO_TEST_SRC_WAVE_PINK_NOISE,
   GST_AUDIO_TEST_SRC_WAVE_SINE_TAB,
   GST_AUDIO_TEST_SRC_WAVE_TICKS,
   GST_AUDIO_TEST_SRC_WAVE_GAUSSIAN_WHITE_NOISE,
   GST_AUDIO_TEST_SRC_WAVE_RED_NOISE,
   GST_AUDIO_TEST_SRC_WAVE_BLUE_NOISE,
   GST_AUDIO_TEST_SRC_WAVE_VIOLET_NOISE
}
*/
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

fn connect_decodebin_pad(src_pad: &gst::Pad, sink: &gst::Element) -> Result<(), Error> {
    let sinkpad = get_static_pad(&sink, "sink")?;
    src_pad.link(&sinkpad).into_result()?;

    Ok(())
}

fn make_fec_encoder(percentage: u32, percentage_important: u32) -> Result<gst::Element, Error> {
    let fecenc = make_element("rtpulpfecenc", None)?;

    fecenc.set_property("pt", &100u32.to_value())?;
    fecenc.set_property("multipacket", &true.to_value())?;
    fecenc.set_property("percentage", &percentage.to_value())?;
    fecenc.set_property("percentage_important", &percentage_important.to_value())?;

    Ok(fecenc)
}

fn example_main() -> Result<(), Error> {
    gst::init()?;
    let args: Vec<_> = env::args().collect();

    if args.len() != 9 {
        return Err(Error::from(UsageError(args[0].clone())));
    }

    let address = &args[1].parse::<String>()?;
    let port = args[2].parse::<i32>()?;
    let opus_bitrate = args[3].parse::<i32>()?;
    let opus_frame_size = args[4].parse::<i32>()?;
    let percentage = args[5].parse::<u32>()?;
    let percentage_important = args[6].parse::<u32>()?;
    let wave = args[7].parse::<i32>()?;
    let freq = args[8].parse::<f64>()?;

    let pipeline = gst::Pipeline::new(None);
    let rtpbin = make_element("rtpbin", None)?;
    let audiotestsrc = make_element("audiotestsrc", None)?;
    let audioconvert = make_element("audioconvert", None)?;
    let opusenc = make_element("opusenc", None)?;
    let rtpopuspay = make_element("rtpopuspay", None)?;
    let udpsink = make_element("udpsink", None)?;

    pipeline.add_many(&[&audiotestsrc, &audioconvert, &opusenc, &rtpopuspay, &rtpbin, &udpsink])?;
    //Check if sink needs to be connected later
  //  gst::Element::link_many(&[&audiotestsrc, &audioconvert, &opusenc, &rtpopuspay, &udpsink])?;

    audioconvert.link(&opusenc)?;
    opusenc.link(&rtpopuspay)?;
    /*
    let src = make_element("uridecodebin", None)?;
    let conv = make_element("videoconvert", None)?;
    let q1 = make_element("queue", None)?;
    let enc = make_element("vp8enc", None)?;
    let q2 = make_element("queue", None)?;
    let pay = make_element("rtpvp8pay", None)?;
    let rtpbin = make_element("rtpbin", None)?;
    let sink = make_element("udpsink", None)?;

    pipeline.add_many(&[&src, &conv, &q1, &enc, &q2, &pay, &rtpbin, &sink])?;

    conv.link(&q1)?;
    q1.link(&enc)?;
    enc.link(&pay)?;
    pay.link(&q2)?; 
    */
    rtpbin.connect("request-fec-encoder", false, move |values| {
        let rtpbin = values[0].get::<gst::Element>().expect("Invalid argument");

        match make_fec_encoder(percentage, percentage_important) {
            Ok(elem) => Some(elem.to_value()),
            Err(err) => {
                gst_element_error!(
                    rtpbin,
                    gst::LibraryError::Failed,
                    ("Failed to make FEC encoder"),
                    ["{}", err]
                );
                None
            }
        }
    })?;

    let srcpad = get_static_pad(&rtpopuspay, "src")?;
    let sinkpad = get_request_pad(&rtpbin, "send_rtp_sink_0")?;
    srcpad.link(&sinkpad).into_result()?;

    
    let srcpad = get_static_pad(&rtpbin, "send_rtp_src_0")?;
    let sinkpad = get_static_pad(&udpsink, "sink")?;
    srcpad.link(&sinkpad).into_result()?;
    
    let convclone = audioconvert.clone();
 /*   jackaudiosrc.connect_pad_added(move |decodebin, src_pad| {
        match connect_decodebin_pad(&src_pad, &convclone) {
            Ok(_) => (),
            Err(err) => {
                gst_element_error!(
                    decodebin,
                    gst::LibraryError::Failed,
                    ("Failed to link decodebin srcpad"),
                    ["{}", err]
                );
                ()
            }
        }
    });*/
    audiotestsrc.link(&audioconvert)?;

    //Are these linkings necessary for us?
    
  /*  let srcpad = get_static_pad(&audiotestsrc, "src")?;
    let sinkpad = get_request_pad(&rtpbin, "send_rtp_sink_0")?;
    srcpad.link(&sinkpad).into_result()?;

    
    let srcpad = get_static_pad(&rtpbin, "send_rtp_src_0")?;
    let sinkpad = get_static_pad(&udpsink, "sink")?;
    srcpad.link(&sinkpad).into_result()?;
    *//*
    let convclone = conv.clone();
    src.connect_pad_added(move |decodebin, src_pad| {
        match connect_decodebin_pad(&src_pad, &convclone) {
            Ok(_) => (),
            Err(err) => {
                gst_element_error!(
                    decodebin,
                    gst::LibraryError::Failed,
                    ("Failed to link decodebin srcpad"),
                    ["{}", err]
                );
                ()
            }
        }
    });*/
 //   let caps = gst::Caps::new_simple("audio/x-rtp", &[]);
    let wave_type = audiotestsrc.get_property("wave").unwrap().type_();
    let wave_as_value = glib::EnumClass::new(wave_type).unwrap().to_value(wave).unwrap();
    let frame_size_type = opusenc.get_property("frame-size").unwrap().type_();
    let frame_size_as_value = glib::EnumClass::new(frame_size_type).unwrap().to_value(opus_frame_size).unwrap();

    audiotestsrc.set_property("wave", &wave_as_value)?;
    audiotestsrc.set_property("freq", &freq.to_value())?;
    opusenc.set_property("bitrate", &opus_bitrate.to_value())?;
    opusenc.set_property("frame-size", &frame_size_as_value)?;
    udpsink.set_property("host", &address.to_value())?;
    udpsink.set_property("sync", &true.to_value())?;
    udpsink.set_property("port", &port.to_value())?;
//    src.set_property("caps", &video_caps.to_value())?;
 //   src.set_property("uri", &uri.to_value())?;

    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    let ret = pipeline.set_state(gst::State::Playing);
    assert_ne!(ret, gst::StateChangeReturn::Failure);

    while let Some(msg) = bus.timed_pop(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
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
                        "server-playing",
                    );
                },
                None => (),
            },
            _ => (),
        }
    }

    let ret = pipeline.set_state(gst::State::Null);
    assert_ne!(ret, gst::StateChangeReturn::Failure);

    Ok(())
}

fn main() {
    match common::run(example_main) {
        Ok(r) => r,
        Err(e) => eprintln!("Error! {}", e),
    }
}
