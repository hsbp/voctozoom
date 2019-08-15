use std::io;
use std::io::{Read,Write};
use std::io::{BufReader, BufWriter};

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
        match bw.write_all(&frame) {
            Err(e) => match e.kind() {
                io::ErrorKind::BrokenPipe => return (),
                _ => panic!("Can't write frame: {}", e),
            }
            Ok(()) => ()
        }
    }
}
