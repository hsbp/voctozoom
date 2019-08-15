use std::io;
use std::io::{Read,Write};
use std::io::{BufReader, BufWriter};

fn main() {
    let stdin = io::stdin();
    let mut br = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut bw = BufWriter::new(stdout.lock());
    let mut frame: [u8; 1280 * 720 * 3] = [0; 1280 * 720 * 3];
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
