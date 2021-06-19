use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Seek, SeekFrom},
    str::FromStr,
    sync::mpsc::Sender,
    thread::JoinHandle,
};

use rouille::{router, Request, Response, Server};

fn new_upload() -> Response {
    let upload_id = rand::random::<[u8; 16]>()
        .iter()
        .map(|x| {
            if *x < 16 {
                format!("0{:X}", x)
            } else {
                format!("{:X}", x)
            }
        })
        .collect::<String>();
    File::create(&upload_id).unwrap();
    Response::redirect_307(format!("/upload/{}", upload_id))
}

fn upload_file(request: &Request, upload_id: &str) -> Response {
    let pairs = form_urlencoded::parse(&request.raw_query_string().as_bytes());
    let params = pairs.collect::<HashMap<_, _>>();
    let mut file = OpenOptions::new().write(true).open(upload_id).unwrap();
    if let Some(pos_str) = params.get("position") {
        let pos = FromStr::from_str(pos_str).unwrap();
        file.seek(SeekFrom::Start(pos)).unwrap();
    }
    let mut buf = BufWriter::new(file);
    let mut body = request.data().unwrap();
    io::copy(&mut body, &mut buf).unwrap();
    Response::empty_204()
}

fn file_size(upload_id: &str) -> Response {
    let meta = fs::metadata(&upload_id).unwrap();
    Response::text(format!("{}", meta.len()))
}

fn handler(request: &Request) -> Response {
    router!(request,
        (POST) (/new) => {new_upload()},
        (POST) (/upload/{upload_id: String}) => {upload_file(request, &upload_id)},
        (GET) (/upload/{upload_id: String}/size) => {file_size(&upload_id)},
        _ => Response::empty_404(),
    )
}

fn new_server() -> (JoinHandle<()>, Sender<()>) {
    let server = Server::new("0.0.0.0:80", handler).expect("Failed to start server");
    server.stoppable()
}

fn main() {
    let (handle, _) = new_server();
    handle.join().unwrap();
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Cursor, Read, Seek},
        thread::{self, sleep},
        time::Duration,
    };

    use super::*;

    fn send_file(target: &str, position: u64) {
        let mut read = Cursor::new(vec![0x10; 100_000_000]);
        read.seek(SeekFrom::Start(position)).unwrap();
        let resp = ureq::post(&target)
            .set("Content-Type", "application/octet-stream")
            .send(read);
        println!("URL: {} Status: {}", resp.get_url(), resp.status(),);
    }

    #[test]
    fn test_main() {
        let (handle, sender) = new_server();

        let resp = ureq::post("http://localhost/new").call();
        let location = resp.header("Location").unwrap();
        let file_name = &location[8..];
        let target = format!("http://localhost{}", location);
        let target2 = target.clone();
        let target3 = target.clone();

        println!(
            "URL: {} Status: {} Target {}",
            resp.get_url(),
            resp.status(),
            &target,
        );

        let t1 = thread::spawn(move || send_file(&target, 0));
        sleep(Duration::from_millis(50));
        let t2 = thread::spawn(move || {
            let resp = ureq::get(&format!("{}/size", &target2)).call();
            let position: u64 = FromStr::from_str(&resp.into_string().unwrap()).unwrap();
            send_file(&format!("{}?position={}", &target2, &position), position)
        });
        t1.join().unwrap();
        let t3 = thread::spawn(move || {
            let position = 50_000_000;
            send_file(&format!("{}?position={}", &target3, &position), position)
        });
        t2.join().unwrap();
        t3.join().unwrap();

        let mut file = File::open(&file_name).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        assert_eq!(data.len(), 100_000_000);

        fs::remove_file(file_name).unwrap();
        sender.send(()).unwrap();
        handle.join().unwrap();
    }
}
