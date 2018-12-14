use std::sync::mpsc;
use std::thread;

use failure::{err_msg, Error};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_base::BaseSrcExt;

#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<gst::buffer::Buffer>,
}

unsafe impl Sync for Recorder {}

impl Recorder {
    pub fn new() -> Self {
        let (sender, recv) = mpsc::channel();

        let pipeline = gst::Pipeline::new(None);
        let appsrc = gst::ElementFactory::make("appsrc", None).expect("Failed to create GStreamer AppSrc");
        let rtph264depay = gst::ElementFactory::make("rtph264depay", None).expect("Failed to create GStreamer rtph264depay");
        let h264parse = gst::ElementFactory::make("h264parse", None).expect("Failed to create GStreamer h264parse");
        let mp4mux = gst::ElementFactory::make("mp4mux", None).expect("Failed to create GStreamer mp4mux");
        let filesink = gst::ElementFactory::make("filesink", None).expect("Failed to create GStreamer filesink");

        let caps = gst::Caps::new_simple(
            "application/x-rtp",
            &[
                ("media", &"video"),
                ("encoding-name", &"H264"),
                ("payload", &96),
                ("clock-rate", &90000),
            ],
        );

        {
            let elems = [&appsrc, &rtph264depay, &h264parse, &mp4mux, &filesink];

            pipeline
                .add_many(&elems)
                .expect("failed to add elems to pipeline");
            gst::Element::link_many(&elems).expect("failed to link elems in pipeline");
        }

        let appsrc = appsrc
            .downcast::<gst_app::AppSrc>()
            .expect("failed downcast to AppSrc");

        appsrc.set_caps(Some(&caps));
        appsrc.set_stream_type(gst_app::AppStreamType::Stream);
        appsrc.set_format(gst::Format::Time);
        appsrc.set_live(true);
        appsrc.set_do_timestamp(true);

        filesink
            .set_property("location", &"test.mp4".to_value())
            .expect("failed to set location prop on filesink?!");

        let res = pipeline.set_state(gst::State::Playing);
        assert_ne!(res, gst::StateChangeReturn::Failure);

        thread::spawn(move || {
            for buf in recv.iter() {
                let res = appsrc.push_buffer(buf);
                if res != gst::FlowReturn::Ok {
                    janus_err!("[CONFERENCE] Error pushing buffer to AppSrc: {:?}", res);
                };
            }

            let res = appsrc.end_of_stream();
            if res != gst::FlowReturn::Ok {
                janus_err!("[CONFERENCE] Error trying to finish stream: {:?}", res);
            }

            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);
            thread::sleep(::std::time::Duration::from_secs(10));
            let res = pipeline.set_state(gst::State::Null);
            assert_ne!(res, gst::StateChangeReturn::Failure);

            janus_info!("end of record");
        });

        Self { sender }
    }

    pub fn record(&self, buf: &[u8]) -> Result<(), Error> {
        let mut gbuf =
            gst::buffer::Buffer::with_size(buf.len()).ok_or(err_msg("Failed to init GBuffer"))?;

        {
            let gbuf = gbuf.get_mut().unwrap();
            gbuf.copy_from_slice(0, buf).map_err(|copied| {
                format_err!(
                    "Failed to copy buf into GBuffer: copied {} out of {} bytes",
                    copied,
                    buf.len()
                )
            })?;
        }

        self.sender.send(gbuf).map_err(|err| Error::from(err))
    }
}
