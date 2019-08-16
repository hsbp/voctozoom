extern crate image;

use std::io;
use std::io::{Read,Write};
use std::sync::{Arc, Mutex,MutexGuard};
use std::thread;

use image::{ImageBuffer,RgbImage,ImageRgb8,FilterType,DynamicImage};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const BYTES_PER_PIXEL: usize = 3;
const BYTES_PER_FRAME: usize = WIDTH * HEIGHT * BYTES_PER_PIXEL;

struct Crop {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

fn main() {
    let stdin = io::stdin();
    let mut sil = stdin.lock();
    let stdout = io::stdout();
    let mut sol = stdout.lock();
    let mut frame: [u8; BYTES_PER_FRAME] = [0; BYTES_PER_FRAME];

    let crop = Arc::new(Mutex::new(Crop { x: 0, y: 0, w: WIDTH as u16, h: HEIGHT as u16 }));
    let crop_read = crop.clone();
    let crop_write = crop.clone();

    thread::spawn(move || {
        // TODO replace with socket based remote updates
        for i in 0..10 {
            thread::sleep_ms(1000);
            let mut params = crop_write.lock().expect("Cannot lock crop parameters for writing");
            params.x = i * 50;
            params.y = i * 25;
            params.w = WIDTH  as u16 - params.x * 2;
            params.h = HEIGHT as u16 - params.y * 2;
        }
    });

    loop {
        match sil.read_exact(& mut frame) {
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => return (),
                _ => panic!("Can't read frame: {}", e),
            }
            Ok(()) => ()
        }

        let ib: RgbImage = ImageBuffer::from_raw(WIDTH as u32, HEIGHT as u32, frame.to_vec()).expect("Cannot create ImageBuffer");
        let cropped = my_crop(ImageRgb8(ib), crop_read.lock().expect("Cannot lock crop parameters for reading"));
        let resized = cropped.resize_exact(WIDTH as u32, HEIGHT as u32, FilterType::CatmullRom);
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

fn my_crop(mut img: DynamicImage, p: MutexGuard<Crop>) -> DynamicImage {
    return img.crop(p.x as u32, p.y as u32, p.w as u32, p.h as u32);
}
