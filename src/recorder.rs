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

            let output_codec = {
                let best = input.streams().best(ffmpeg::media::Type::Video).unwrap();
                ffmpeg::encoder::find(best.codec().id()).expect("failed to deduce output codec")
            };

            let path = Path::new("test.mp4");
            let mut output = ffmpeg::format::output_as(&path, "mp4").expect("Failed to create output");
            output.add_stream(output_codec).expect("failed to add stream");

            for (stream, mut packet) in input.packets() {
                packet.set_stream(stream.index());
                packet.write(&mut output).expect("failed to write packet");
            }
        });

        Self {
            socket
        }
    }

    pub fn relay(&self, buf: &[u8]) {
        self.socket.send_to(buf, "127.0.0.1:20001").expect("Failed to send UDP packet");
    }
}
