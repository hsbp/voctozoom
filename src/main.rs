use std::env;
use std::io;
use std::io::{BufRead,BufReader,BufWriter,Read,Stdin,Write};
use std::net::TcpListener;
use std::sync::{mpsc,Arc,Mutex,MutexGuard};
use std::thread;
use std::process::{Command, Stdio, Child};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const BYTES_PER_PIXEL: usize = 3;
const BYTES_PER_FRAME: usize = WIDTH * HEIGHT * BYTES_PER_PIXEL;
const FULL_CROP: Crop = Crop { x: 0, y: 0, w: WIDTH as u16, h: HEIGHT as u16 };
const DEFAULT_PORT: &str = "20000";

#[derive(Copy, Clone, PartialEq, Eq)]
struct Crop {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

fn parse_zoom_to(parts: Vec<&str>) -> Result<Crop, &str> {
    let params: Vec<&str> = match parts.get(1) {
        Some(param) => param.split('+').collect(),
        None => { return Err("Missing parameter"); }
    };
    let resolution: Vec<Result<u16, std::num::ParseIntError>> = params[0].split('x').map(|s| s.parse::<u16>()).collect();
    let offsets: Vec<Result<u16, std::num::ParseIntError>> = params.iter().skip(1).map(|s| s.parse::<u16>()).collect();
    if resolution.len() != 2 || offsets.len() != 2 || resolution.iter().chain(offsets.iter()).any(Result::is_err) {
        return Err("Incorrect resolution syntax");
    }
    let w = *(resolution[0].as_ref().unwrap());
    let h = *(resolution[1].as_ref().unwrap());
    let x = *(offsets[0].as_ref().unwrap());
    let y = *(offsets[1].as_ref().unwrap());
    if w as usize + x as usize > WIDTH {
        return Err("Viewport is outside the screen in horizontal direction");
    }
    if h as usize + y as usize > HEIGHT {
        return Err("Viewport is outside the screen in vertical direction");
    }
    Ok(Crop{ x, y, w, h })
}

fn main() {
    let mut args = env::args();
    let execname = args.next();
    let port = match args.next() {
        Some(p) => p,
        None => String::from(DEFAULT_PORT),
    };
    if port == "-h" {
        eprintln!("Usage: {} [port]", execname.unwrap());
        return;
    }
    let bind_addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&bind_addr).unwrap_or_else(|_| panic!("Cannot bind to {}", bind_addr));

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut sol = stdout.lock();
    let mut frame: [u8; BYTES_PER_FRAME] = [0; BYTES_PER_FRAME];

    let crop_read = Arc::new(Mutex::new(FULL_CROP));
    let crop_write = crop_read.clone();
    let mut scaler: Option<Child> = None;
    let mut scaler_w = FULL_CROP.w;
    let mut scaler_h = FULL_CROP.h;

    let (img_req_tx,  img_req_rx)  = mpsc::channel();
    let (img_resp_tx, img_resp_rx): (mpsc::Sender<Option<Vec<u8>>>, mpsc::Receiver<Option<Vec<u8>>>) = mpsc::channel();

    thread::spawn(move || {
        'streams: for accepted in listener.incoming() {
            let stream = accepted.expect("Cannot accept connection");
            let br = BufReader::new(&stream);
            let mut bw = BufWriter::new(&stream);
            for line in br.lines() {
                let payload = match line {
                    Err(_) => continue 'streams,
                    Ok(p) => p,
                };
                let parts: Vec<&str> = payload.trim().split(" ").collect();
                if parts[0] == "zoom_to" {
                    let reply = match parse_zoom_to(parts) {
                        Ok(new_crop) => {
                            let mut params = crop_write.lock().expect("Cannot lock crop parameters for writing");
                            *params = new_crop;
                            "OK"
                        },
                        Err(msg) => msg
                    };
                    bw.write(&format!("{}\n", reply).into_bytes());
                } else if parts[0] == "get_resolution" {
                    bw.write(&format!("{}x{}\n", WIDTH, HEIGHT).into_bytes());
                } else if parts[0] == "get_image" {
                    img_req_tx.send(()).expect("Cannot request image");
                    while let Some(buf) = img_resp_rx.recv().expect("Cannot receive image") {
                        bw.write_all(&buf).expect("Cannot forward image");
                    }
                } else {
                    bw.write(b"Unknown command\n");
                }
                bw.flush();
            }
        }
    });

    loop {
        if is_full_screen(crop_read.lock().expect("Cannot lock crop parameters for checking")) {
            if let Some(ffmpeg) = scaler {
                let out = ffmpeg.wait_with_output().expect("Failed to wait on old scaler");
                scaler = None;
                if check_errors_and_eof(sol.write_all(&out.stdout), "Can't write old scaler output frame") { return; }
                scaler_w = FULL_CROP.w;
                scaler_h = FULL_CROP.h;
            }
            let mut sil = stdin.lock();
            if check_errors_and_eof(sil.read_exact(& mut frame),  "Can't read frame") { return; }
            if img_req_rx.try_recv().is_ok() {
                img_resp_tx.send(Some(frame.to_vec())).expect("Cannot send frame to socket handler");
                img_resp_tx.send(None).expect("Cannot send end-of-frame to socket handler");
            }
            if check_errors_and_eof(sol.write_all (&     frame), "Can't write frame") { return; }
        } else {
            let rc_tx = match img_req_rx.try_recv() {
                Ok(_) => Some(&img_resp_tx),
                _ => None
            };
            let cropped = match read_cropped(&crop_read, &stdin, rc_tx) {
                Some(c) => c,
                None => return
            };

            {
                let crop_check = crop_read.lock().expect("Cannot lock crop parameters for checking");
                if scaler_w != crop_check.w || scaler_h != crop_check.h {
                    if let Some(ffmpeg) = scaler {
                        let out = ffmpeg.wait_with_output().expect("Failed to wait on old scaler");
                        if check_errors_and_eof(sol.write_all(&out.stdout), "Can't write old scaler output frame") { return; }
                    }
                    scaler_w = crop_check.w;
                    scaler_h = crop_check.h;
                    scaler = Some(Command::new("ffmpeg")
                        .arg("-loglevel").arg("quiet")
                        .arg("-f").arg("rawvideo")
                        .arg("-pixel_format").arg("rgb24")
                        .arg("-s").arg(format!("{}x{}", scaler_w, scaler_h))
                        .arg("-i").arg("-")
                        .arg("-filter:v").arg(format!("scale={}:{}", WIDTH, HEIGHT))
                        .arg("-f").arg("rawvideo")
                        .arg("-pixel_format").arg("rgb24")
                        .arg("-")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("failed to execute ffmpeg"));
                }
            }

            {
                let ffmpeg = scaler.as_mut().unwrap();
                {
                    let mut read_offset: usize = 0;
                    let mut write_offset: usize = 0;
                    let ffmpeg_stdin = ffmpeg.stdin.as_mut().expect("failed to get stdin");
                    let ffmpeg_stdout = ffmpeg.stdout.as_mut().expect("failed to get stdout");
                    let expected_read = (scaler_w * scaler_h) as usize * BYTES_PER_PIXEL;

                    while read_offset < expected_read || write_offset < BYTES_PER_FRAME {
                        if read_offset < expected_read {
                            let bytes_written = ffmpeg_stdin.write(&cropped[read_offset..]).expect("failed to write frame to scaler");
                            read_offset += bytes_written;
                        }

                        if write_offset < BYTES_PER_FRAME {
                            let read_bytes = ffmpeg_stdout.read(& mut frame).expect("Can't read frames from scaler");
                            if read_bytes == 0 { continue; }
                            write_offset += read_bytes;

                            if check_errors_and_eof(sol.write_all(&frame[0..read_bytes]), "Can't write frame") { return; }
                        }
                    }
                }
            }
        }
    }
}

fn read_cropped(p: & Arc<Mutex<Crop>>, stdin: & Stdin, img_resp_tx: Option<& mpsc::Sender<Option<Vec<u8>>>>) -> Option<Vec<u8>> {
    let crop = p.lock().expect("Cannot lock crop parameters for reading");

    let mut skip_front: Vec<u8> = vec![0; (crop.y as usize * WIDTH + crop.x as usize) * BYTES_PER_PIXEL];
    let mut skip_line: Vec<u8> = vec![0; (WIDTH - crop.w as usize) * BYTES_PER_PIXEL];
    let mut line: Vec<u8> = vec![0; (crop.w as usize) * BYTES_PER_PIXEL];
    let mut skip_back: Vec<u8> = vec![0; (WIDTH - crop.w as usize - crop.x as usize + (HEIGHT - crop.y as usize - crop.h as usize) * WIDTH) * BYTES_PER_PIXEL];

    let mut frame: Vec<u8> = Vec::with_capacity(crop.w as usize * crop.h as usize * BYTES_PER_PIXEL);

    let mut sil = stdin.lock();
    if check_errors_and_eof(sil.read_exact(& mut skip_front),    "Can't read frame") { return None; }
    if check_errors_and_eof(sil.read_exact(& mut line),          "Can't read frame") { return None; }
    if let Some(tx) = img_resp_tx {
        tx.send(Some(skip_front.to_vec())).expect("Cannot send frame to socket handler");
        tx.send(Some(line.to_vec())).expect("Cannot send frame to socket handler");
    }

    frame.extend(line.iter().cloned());
    for _ in 1..crop.h {
        if check_errors_and_eof(sil.read_exact(& mut skip_line), "Can't read frame") { return None; }
        if check_errors_and_eof(sil.read_exact(& mut line),      "Can't read frame") { return None; }
        frame.extend(line.iter().cloned());
        if let Some(tx) = img_resp_tx {
            tx.send(Some(skip_line.to_vec())).expect("Cannot send frame to socket handler");
            tx.send(Some(line.to_vec())).expect("Cannot send frame to socket handler");
        }
    }
    if check_errors_and_eof(sil.read_exact(& mut skip_back),     "Can't read frame") { return None; }
    if let Some(tx) = img_resp_tx {
        tx.send(Some(skip_back)).expect("Cannot send frame to socket handler");
        tx.send(None).expect("Cannot send end-of-frame to socket handler");
    }

    Some(frame)
}

fn check_errors_and_eof<T>(result: Result<T, std::io::Error>, panic_msg: &str) -> bool {
    match result {
        Err(e) => match e.kind() {
            io::ErrorKind::UnexpectedEof => true,
            io::ErrorKind::BrokenPipe => true,
            _ => panic!("{}: {}", panic_msg, e),
        }
        Ok(_) => false
    }
}

fn is_full_screen(p: MutexGuard<Crop>) -> bool {
    *p == FULL_CROP
}
