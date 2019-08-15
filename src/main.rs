extern crate image;

use std::io;
use std::io::{Read,Write};
use std::io::{BufReader, BufWriter};

use image::{ImageBuffer,RgbImage,ImageRgb8,FilterType};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const BYTES_PER_PIXEL: usize = 3;
const BYTES_PER_FRAME: usize = WIDTH * HEIGHT * BYTES_PER_PIXEL;

fn main() {
    let stdin = io::stdin();
    let mut br = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut bw = BufWriter::new(stdout.lock());
    let mut frame: [u8; BYTES_PER_FRAME] = [0; BYTES_PER_FRAME];
    loop {
        match br.read_exact(& mut frame) {
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => return (),
                _ => panic!("Can't read frame: {}", e),
            }
            Ok(()) => ()
        }

        let ib: RgbImage = ImageBuffer::from_raw(WIDTH as u32, HEIGHT as u32, frame.to_vec()).expect("Cannot create ImageBuffer");
        let cropped = ImageRgb8(ib).crop((WIDTH / 4) as u32, (HEIGHT / 4) as u32,
                (WIDTH / 2) as u32, (HEIGHT / 2) as u32); // TODO parameterize this
        let resized = cropped.resize_exact(WIDTH as u32, HEIGHT as u32, FilterType::CatmullRom);
        let resized8 = resized.as_rgb8().expect("Cannot convert to RGB8");

        match bw.write_all(&resized8) {
            Err(e) => match e.kind() {
                io::ErrorKind::BrokenPipe => return (),
                _ => panic!("Can't write frame: {}", e),
            }
            Ok(()) => ()
        }
    }
}
