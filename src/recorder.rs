use std::thread;
use std::sync::mpsc;

use gstreamer::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_base::BaseSrcExt;

#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<gst::buffer::Buffer>
}

unsafe impl Sync for Recorder {}

impl Recorder {
    pub fn new() -> Self {
        let (sender, recv) = mpsc::channel();

        let pipeline = gst::Pipeline::new(None);
        let appsrc = gst::ElementFactory::make("appsrc", None).unwrap();
        let rtph264depay = gst::ElementFactory::make("rtph264depay", None).unwrap();
        let h264parse = gst::ElementFactory::make("h264parse", None).unwrap();
        let mp4mux = gst::ElementFactory::make("mp4mux", None).unwrap();
        let filesink = gst::ElementFactory::make("filesink", None).unwrap();

        let caps = gst::Caps::new_simple(
            "application/x-rtp",
            &[
                ("media", &"video"),
                ("encoding-name", &"H264"),
                ("payload", &96),
                ("clock-rate", &90000)
            ]
        );

        {
            let elems = [
                &appsrc,
                &rtph264depay,
                &h264parse,
                &mp4mux,
                &filesink
            ];

            pipeline.add_many(&elems).expect("failed to add elems to pipeline");
            gst::Element::link_many(&elems).expect("failed to link elems in pipeline");
        }

        let appsrc = appsrc.downcast::<gst_app::AppSrc>().expect("failed downcast to AppSrc");

        appsrc.set_caps(Some(&caps));
        appsrc.set_stream_type(gst_app::AppStreamType::Stream);
        appsrc.set_format(gst::Format::Time);
        appsrc.set_live(true);
        appsrc.set_do_timestamp(true);

        filesink.set_property("location", &"test.mp4".to_value()).expect("failed to set location prop on filesink?!");

        pipeline.set_state(gst::State::Playing);

        thread::spawn(move || {
            for buf in recv.iter() {
                appsrc.push_buffer(buf);
            }

            appsrc.end_of_stream();
            
            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);
            thread::sleep(::std::time::Duration::from_secs(10));
            pipeline.set_state(gst::State::Null);

            janus_info!("end of record");
        });

        Self {
            sender
        }
    }

    pub fn record(&self, buf: &[u8]) {
        let mut gbuf = gst::buffer::Buffer::with_size(buf.len()).unwrap();

        {
            let gbuf = gbuf.get_mut().unwrap();
            gbuf.copy_from_slice(0, buf).expect("failed to copy buf");
        }

        self.sender.send(gbuf).expect("failed to send buf to recorder pipeline");
    }
}
