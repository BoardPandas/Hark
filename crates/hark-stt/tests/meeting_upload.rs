//! HTTP boundary test: archive bytes remain stereo and reach the provider unchanged.
use hark_stt::meeting::{deepgram_final_pass_encoded, MeetingEncoding};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn mp3_upload_uses_multichannel_mime_and_unchanged_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = b"synthetic stereo MP3 bytes";
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            request.push_str(&line);
        }
        let lower = request.to_lowercase();
        assert!(lower.contains("multichannel=true"));
        assert!(lower.contains("content-type: audio/mpeg"));
        assert!(lower.contains(&format!("content-length: {}", payload.len())));
        let mut body = vec![0; payload.len()];
        reader.read_exact(&mut body).unwrap();
        assert_eq!(body, payload);
        let json = r#"{"results":{"utterances":[{"channel":1,"speaker":0,"start":0,"end":1,"transcript":"fixture"}]}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        )
        .unwrap();
    });
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let segments = deepgram_final_pass_encoded(
        &client,
        &format!("http://{addr}"),
        "test-key",
        Box::new(std::io::Cursor::new(payload)),
        payload.len() as u64,
        &[],
        1000,
        MeetingEncoding::Mp3,
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].channel, 1);
}
