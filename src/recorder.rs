use std::net::UdpSocket;
use std::path::Path;
use std::thread;

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
            let mut decoder = input.streams().next().unwrap().codec().decoder().video().unwrap();

            let output_codec = ffmpeg::encoder::find(ffmpeg::codec::Id::H264)
                .expect("failed to deduce output codec");

            let path = Path::new("test.mp4");
            let mut output =
                ffmpeg::format::output_as(&path, "mp4").expect("Failed to create output");

            let mut encoder = {
                let mut out_stream = output
                    .add_stream(output_codec)
                    .expect("failed to add stream");
                let mut encoder = out_stream.codec().encoder().video().unwrap();

                out_stream.set_time_base((1, 30));
                encoder.set_frame_rate(decoder.frame_rate());

                unsafe {
                    (*encoder.as_mut_ptr()).time_base.den = 30;
                    (*encoder.as_mut_ptr()).time_base.num = 1;
                    janus_info!("{} {}", (*encoder.as_mut_ptr()).time_base.num, (*encoder.as_mut_ptr()).time_base.den);
                }

                encoder.open_as(output_codec).unwrap()
            };

            let mut decoded = ffmpeg::util::frame::Video::empty();
            let mut encoded = ffmpeg::Packet::empty();

            for (stream, mut packet) in input.packets() {
                packet.set_stream(stream.index());

                decoder.decode(&packet, &mut decoded);
                let ts = decoded.timestamp();
                decoded.set_pts(ts);

                encoder.encode(&decoded, &mut encoded);
                encoded.set_stream(0);
                encoded.write_interleaved(&mut output);
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
