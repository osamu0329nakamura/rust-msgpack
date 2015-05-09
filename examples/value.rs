extern crate msgpack;

use std::fs::File;
use std::env::args;
use std::io::Read;

fn main() {
    let mut contents = vec![];
    File::open(args().next().expect("")).unwrap().read_to_end(&mut contents).ok().unwrap();
  println!("{:?}", contents);

/* todo
  let a: msgpack::Value = msgpack::from_msgpack(contents.as_slice()).ok().unwrap();
  println!("{:?}", a);
*/
}
