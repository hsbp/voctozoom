extern crate image;

use std::io;
use std::io::{BufRead,BufReader,BufWriter,Read,Stdin,Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex,MutexGuard};
use std::thread;

use image::{ImageBuffer,RgbImage,ImageRgb8,FilterType,DynamicImage};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const BYTES_PER_PIXEL: usize = 3;
const BYTES_PER_FRAME: usize = WIDTH * HEIGHT * BYTES_PER_PIXEL;
const FULL_CROP: Crop = Crop { x: 0, y: 0, w: WIDTH as u16, h: HEIGHT as u16 };

#[derive(Copy, Clone, PartialEq, Eq)]
struct Crop {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut sol = stdout.lock();
    let mut frame: [u8; BYTES_PER_FRAME] = [0; BYTES_PER_FRAME];

    let crop = Arc::new(Mutex::new(FULL_CROP));
    let crop_read = crop.clone();
    let crop_write = crop.clone();

    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:20000").expect("Cannot bind to port 20000");

        'streams: for accepted in listener.incoming() {
            let stream = accepted.expect("Cannot accept connection");
            let br = BufReader::new(&stream);
            let mut bw = BufWriter::new(&stream);
            'lines: for line in br.lines() {
                let payload = match line {
                    Err(_) => continue 'streams,
                    Ok(p) => p,
                };
                let parts: Vec<&str> = payload.trim().split(" ").collect();
                if parts[0] == "zoom_to" {
                    if parts.len() < 2 {
                        bw.write(b"Missing parameter\n");
                        bw.flush();
                        continue 'lines;
                    }
                    let params: Vec<&str> = parts[1].split("+").collect();
                    let resolution: Vec<Result<u16, std::num::ParseIntError>> = params[0].split("x").map(|s| s.parse::<u16>()).collect();
                    let offsets: Vec<Result<u16, std::num::ParseIntError>> = params.iter().skip(1).map(|s| s.parse::<u16>()).collect();
                    if resolution.len() != 2 || offsets.len() != 2 || resolution.iter().chain(offsets.iter()).any(Result::is_err) {
                        bw.write(b"Incorrect resolution syntax\n");
                        bw.flush();
                        continue 'lines;
                    }
                    let w = *(resolution[0].as_ref().unwrap());
                    let h = *(resolution[1].as_ref().unwrap());
                    let x = *(offsets[0].as_ref().unwrap());
                    let y = *(offsets[1].as_ref().unwrap());
                    if w as usize + x as usize > WIDTH {
                        bw.write(b"Viewport is outside the screen in horizontal direction\n");
                        bw.flush();
                        continue 'lines;
                    }
                    if h as usize + y as usize > HEIGHT {
                        bw.write(b"Viewport is outside the screen in vertical direction\n");
                        bw.flush();
                        continue 'lines;
                    }
                    let mut params = crop_write.lock().expect("Cannot lock crop parameters for writing");
                    params.x = x;
                    params.y = y;
                    params.w = w;
                    params.h = h;
                    bw.write(b"OK\n");
                    bw.flush();
                } else {
                    bw.write(b"Unknown command\n");
                    bw.flush();
                }
            }
        }
    });

    loop {
        if is_full_screen(crop_read.lock().expect("Cannot lock crop parameters for checking")) {
            let mut sil = stdin.lock();
            match sil.read_exact(& mut frame) {
                Err(e) => match e.kind() {
                    io::ErrorKind::UnexpectedEof => return (),
                    _ => panic!("Can't read frame: {}", e),
                }
                Ok(()) => ()
            }

            match sol.write_all(&frame) {
                Err(e) => match e.kind() {
                    io::ErrorKind::BrokenPipe => return (),
                    _ => panic!("Can't write frame: {}", e),
                }
                Ok(()) => ()
            }
        } else {
            // TODO measure cropping here vs. cropping during read
            let resized = match read_cropped(&crop_read, &stdin) {
                Some(cropped) => cropped.resize_exact(WIDTH as u32, HEIGHT as u32, FilterType::CatmullRom),
                None => return ()
            };

            let resized8 = resized.as_rgb8().expect("Cannot convert to RGB8");

            match sol.write_all(&resized8) {
                Err(e) => match e.kind() {
                    io::ErrorKind::BrokenPipe => return (),
                    _ => panic!("Can't write frame: {}", e),
                }
                Ok(()) => ()
            }
        }
    }
}

fn read_cropped(p: & Arc<Mutex<Crop>>, stdin: & Stdin) -> Option<DynamicImage> {
    let crop = p.lock().expect("Cannot lock crop parameters for reading");

    let mut skip_front: Vec<u8> = vec![0; (crop.y as usize * WIDTH + crop.x as usize) * BYTES_PER_PIXEL];
    let mut skip_line: Vec<u8> = vec![0; (WIDTH - crop.w as usize) * BYTES_PER_PIXEL];
    let mut line: Vec<u8> = vec![0; (crop.w as usize) * BYTES_PER_PIXEL];
    let mut skip_back: Vec<u8> = vec![0; (WIDTH - crop.w as usize - crop.x as usize + (HEIGHT - crop.y as usize - crop.h as usize) * WIDTH) * BYTES_PER_PIXEL];

    let mut frame: Vec<u8> = Vec::with_capacity(crop.w as usize * crop.h as usize * BYTES_PER_PIXEL);

    {
        let mut sil = stdin.lock();
        match sil.read_exact(& mut skip_front) {
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => return None,
                _ => panic!("Can't read frame: {}", e),
            }
            Ok(()) => ()
        }
        match sil.read_exact(& mut line) {
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => return None,
                _ => panic!("Can't read frame: {}", e),
            }
            Ok(()) => ()
        }
        frame.extend(line.iter().cloned());
        for _ in 1..crop.h {
            match sil.read_exact(& mut skip_line) {
                Err(e) => match e.kind() {
                    io::ErrorKind::UnexpectedEof => return None,
                    _ => panic!("Can't read frame: {}", e),
                }
                Ok(()) => ()
            }
            match sil.read_exact(& mut line) {
                Err(e) => match e.kind() {
                    io::ErrorKind::UnexpectedEof => return None,
                    _ => panic!("Can't read frame: {}", e),
                }
                Ok(()) => ()
            }
            frame.extend(line.iter().cloned());
        }
        match sil.read_exact(& mut skip_back) {
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => return None,
                _ => panic!("Can't read frame: {}", e),
            }
            Ok(()) => ()
        }
    }

    let ib: RgbImage = ImageBuffer::from_raw(crop.w as u32, crop.h as u32, frame).expect("Cannot create ImageBuffer");
    return Some(ImageRgb8(ib));
}

fn is_full_screen(p: MutexGuard<Crop>) -> bool {
    return *p == FULL_CROP;
}
