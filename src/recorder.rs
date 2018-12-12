use std::net::UdpSocket;
use std::path::Path;
use std::thread;
use ffmpeg::codec::packet::Mut;

#[derive(Debug)]
pub struct Recorder {
    socket: UdpSocket,
}

impl Recorder {
    pub fn new() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:20000").expect("Failed to bind UDP socket");

        thread::spawn(move || {
            let path = Path::new("test.sdp");
            let opts = dict! {
                "protocol_whitelist" => "file,udp,rtp",
            };
            let mut input = ffmpeg::format::input_with(&path, opts).unwrap();

            let output_codec = ffmpeg::encoder::find(ffmpeg::codec::Id::H264)
                .expect("failed to deduce output codec");

            let opts = dict! {
                "movflags" => "faststart",
                "movflags" => "frag_keyframe+empty_moov",
                "vcodec" => "copy"
            };

            let path = Path::new("test.mp4");
            let mut output =
                ffmpeg::format::output_as_with(&path, "mp4", opts).expect("Failed to create output");

            unsafe {
                let mut out_stream = output
                    .add_stream(output_codec)
                    .expect("failed to add stream");

                ffmpeg::ffi::avcodec_copy_context(out_stream.codec().as_mut_ptr(), input.streams().next().unwrap().codec().as_mut_ptr());

                loop {
                    let mut packet = ffmpeg::packet::Packet::empty();
                    let mut ret = ffmpeg::ffi::av_read_frame(input.as_mut_ptr(), packet.as_mut_ptr());
                    janus_info!("{}", ret);
                    ret = ffmpeg::ffi::av_interleaved_write_frame(output.as_mut_ptr(), packet.as_mut_ptr());
                    janus_info!("{}", ret);
                }
            }
        });

        Self { socket }
    }

    pub fn relay(&self, buf: &[u8]) {
        self.socket
            .send_to(buf, "127.0.0.1:20001")
            .expect("Failed to send UDP packet");
    }
}
