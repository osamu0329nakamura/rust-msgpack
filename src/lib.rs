//! msgpack.org implementation for Rust

#![crate_type = "lib"]
#![allow(unused_must_use, dead_code)]
//#![feature(io, core, rustc_private, custom_derive)]
#![feature(rustc_private, custom_derive)]

extern crate rustc;
extern crate serialize;
extern crate byteorder;
extern crate num;
extern crate rustc_serialize;
use std::io::{BufReader, Read, Write};
use std::io::ErrorKind;
use std::str::from_utf8;
use std::mem;

use rustc::util::num::ToPrimitive;
use serialize::{Encodable, Decodable};

use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian, Error};

pub type Result<T> = std::result::Result<T, byteorder::Error>;

type IOError = std::io::Error;

#[cfg(todo)]
mod rpc;

#[derive(Debug)]
pub enum Value {
    Nil,
    Boolean(bool),
    Integer(i64),
    Unsigned(u64),
    Float(f32),
    Double(f64),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Str(Vec<u8>),
    Binary(Vec<u8>),
    Extended(i8, Vec<u8>)
}

#[inline(always)]
fn read_float(rd: &mut Read) -> Result<f32> {
    rd.read_u32::<BigEndian>().map(|v| unsafe { mem::transmute(v) })
}

#[inline(always)]
fn read_double(rd: &mut Read) -> Result<f64> {
    rd.read_u64::<BigEndian>().map(|v| unsafe { mem::transmute(v) })
}

pub fn _invalid_input(s: &'static str) -> byteorder::Error {
    Error::from(IOError::new(ErrorKind::InvalidInput, s))
}

/// A structure to decode Msgpack from a reader.
pub struct Decoder<R: Read> {
    rd: R,
    next_byte: Option<u8>
}

impl<R: Read> Decoder<R> {
    /// Creates a new Msgpack decoder for decoding from the
    /// specified reader.
    pub fn new(rd: R) -> Decoder<R> {
        Decoder {
            rd: rd,
            next_byte: None
        }
    }
}

impl<'a, R: Read> Decoder<R> {
    fn _peek_byte(&mut self) -> Result<u8> {
        match self.next_byte {
            Some(byte) => Ok(byte),
            None => {
                match self.rd.read_u8() {
                    Ok(byte) => {
                        self.next_byte = Some(byte);
                        Ok(byte)
                    }
                    err => err
                }

            }
        }
    }

    fn _read_byte(&mut self) -> Result<u8> {
        match self.next_byte {
            Some(byte) => {
                self.next_byte = None;
                Ok(byte)
            }
            None => {
                self.rd.read_u8()
            }
        }
    }

    fn _read_unsigned(&mut self) -> Result<u64> {
        let c = try!(self._read_byte());
        match c {
            0x00 ... 0x7f => Ok(c as u64),
            0xcc         => Ok(try!(self.rd.read_u8()) as u64),
            0xcd         => Ok(try!(self.rd.read_u16::<BigEndian>()) as u64),
            0xce         => Ok(try!(self.rd.read_u32::<BigEndian>()) as u64),
            0xcf         => self.rd.read_u64::<BigEndian>(),
            _            => Err(_invalid_input("No unsigned integer"))
        }
    }

    fn _read_signed(&mut self) -> Result<i64> {
        let c = try!(self._read_byte());
        match c {
            0xd0         => Ok(try!(self.rd.read_i8()) as i64),
            0xd1         => Ok(try!(self.rd.read_i16::<BigEndian>()) as i64),
            0xd2         => Ok(try!(self.rd.read_i32::<BigEndian>()) as i64),
            0xd3         => self.rd.read_i64::<BigEndian>(),
            0xe0 ... 0xff => Ok((c as i8) as i64),
            _            => Err(_invalid_input("No signed integer"))
        }
    }

    fn _read_exact(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut v = Vec::with_capacity(len);
        let ref mut rd = self.rd;
        match rd.take(len as u64).read_to_end(&mut v) {
            Ok(size) if size == len => {return Ok(v)}
            Ok(_) => {return Err(Error::UnexpectedEOF)}
            Err(why) => {return Err(Error::from(why))}
        };
    }

    fn _read_raw(&mut self, len: usize) -> Result<Vec<u8>> {
        self._read_exact(len)
    }

    fn _read_str(&mut self, len: usize) -> Result<String> {
        match String::from_utf8(try!(self._read_exact(len))) {
            Ok(s)  => Ok(s),
            Err(_) => Err(_invalid_input("No UTF-8 string"))
        }
    }

    fn _read_vec_len(&mut self) -> Result<usize> {
        let c = try!(self._read_byte());

        match c {
            0x90 ... 0x9f => Ok((c as usize) & 0x0F),
            0xdc         => self.rd.read_u16::<BigEndian>().map(|i| i as usize),
            0xdd         => self.rd.read_u32::<BigEndian>().map(|i| i as usize),
            _            => Err(_invalid_input("Invalid byte code in _read_vec_len"))
        }
    }

    fn _read_map_len(&mut self) -> Result<usize> {
        let c = try!(self._read_byte());
        match c {
            0x80 ... 0x8f => Ok((c as usize) & 0x0F),
            0xde         => self.rd.read_u16::<BigEndian>().map(|i| i as usize),
            0xdf         => self.rd.read_u32::<BigEndian>().map(|i| i as usize),
            _            => Err(_invalid_input("Invalid byte code in _read_map_len"))
        }
    }

    fn decode_array(&mut self, len: usize) -> Result<Value> {
        let mut v = Vec::with_capacity(len);
        for _ in 0 .. len {
            v.push(try!(self.decode_value()));
        }
        Ok(Value::Array(v))
    }

    fn decode_map(&mut self, len: usize) -> Result<Value> {
        let mut v = Vec::with_capacity(len);
        for _ in 0 .. len {
            let a = try!(self.decode_value());
            let b = try!(self.decode_value());
            v.push((a, b));
        }
        Ok(Value::Map(v))
    }

    fn decode_ext(&mut self, len: usize) -> Result<Value> {
        let typ = try!(self.rd.read_i8());
        if typ < 0 {
            return Err(_invalid_input("Reserved type"));
        }
        Ok(Value::Extended(typ, try!(self._read_exact(len))))
    }

    fn decode_value(&mut self) -> Result<Value> {
        let c = try!(self._read_byte());
        match c {
            0xc0         => Ok(Value::Nil),

            0xc1         => Err(_invalid_input("Reserved")),

            0xc2         => Ok(Value::Boolean(false)),
            0xc3         => Ok(Value::Boolean(true)),

            0x00 ... 0x7f => Ok(Value::Unsigned(c as u64)),
            0xcc         => self.rd.read_u8().map(|i| Value::Unsigned(i as u64)),
            0xcd         => self.rd.read_u16::<BigEndian>().map(|i| Value::Unsigned(i as u64)),
            0xce         => self.rd.read_u32::<BigEndian>().map(|i| Value::Unsigned(i as u64)),
            0xcf         => self.rd.read_u64::<BigEndian>().map(|i| Value::Unsigned(i)),

            0xd0         => self.rd.read_i8().map(|i| Value::Integer(i as i64)),
            0xd1         => self.rd.read_i16::<BigEndian>().map(|i| Value::Integer(i as i64)),
            0xd2         => self.rd.read_i32::<BigEndian>().map(|i| Value::Integer(i as i64)),
            0xd3         => self.rd.read_i64::<BigEndian>().map(|i| Value::Integer(i)),
            0xe0 ... 0xff => Ok(Value::Integer((c as i8) as i64)),

            0xca         => read_float(&mut self.rd).map(|i| Value::Float(i)),
            0xcb         => read_double(&mut self.rd).map(|i| Value::Double(i)),

            0xa0 ... 0xbf => self._read_raw((c as usize) & 0x1F).map(|i| Value::Str(i)),
            0xd9         => {
                let l = try!(self.rd.read_u8()) as usize;
                self._read_raw(l).map(|i| Value::Str(i))
            }
            0xda         => {
                let l = try!(self.rd.read_u16::<BigEndian>()) as usize;
                self._read_raw(l).map(|i| Value::Str(i))
            }
            0xdb         => {
                let l = try!(self.rd.read_u32::<BigEndian>()) as usize;
                self._read_raw(l).map(|i| Value::Str(i))
            }

            0xc4         => {
                let l = try!(self.rd.read_u8()) as usize;
                self._read_raw(l).map(|i| Value::Binary(i))
            }

            0xc5         => {
                let l = try!(self.rd.read_u16::<BigEndian>()) as usize;
                self._read_raw(l).map(|i| Value::Binary(i))
            }


            0xc6         => {
                let l = try!(self.rd.read_u32::<BigEndian>()) as usize;
                self._read_raw(l).map(|i| Value::Binary(i))
            }

            0x90 ... 0x9f => self.decode_array((c as usize) & 0x0F),
            0xdc         => { let l = try!(self.rd.read_u16::<BigEndian>()) as usize; self.decode_array(l) },
            0xdd         => { let l = try!(self.rd.read_u32::<BigEndian>()) as usize; self.decode_array(l) },

            0x80 ... 0x8f => self.decode_map((c as usize) & 0x0F),
            0xde         => { let l = try!(self.rd.read_u16::<BigEndian>()) as usize; self.decode_map(l) },
            0xdf         => { let l = try!(self.rd.read_u32::<BigEndian>()) as usize; self.decode_map(l) },

            0xd4         => self.decode_ext(1),
            0xd5         => self.decode_ext(2),
            0xd6         => self.decode_ext(4),
            0xd7         => self.decode_ext(8),
            0xd8         => self.decode_ext(16),
            0xc7         => { let l = try!(self.rd.read_u8()) as usize; self.decode_ext(l) },
            0xc8         => { let l = try!(self.rd.read_u16::<BigEndian>()) as usize; self.decode_ext(l) },
            0xc9         => { let l = try!(self.rd.read_u32::<BigEndian>()) as usize; self.decode_ext(l) },

            // XXX: This is only here to satify Rust's pattern checker.
            _            => unreachable!()
        }
    }


}

impl<R: Read> serialize::Decoder for Decoder<R> {
    type Error = byteorder::Error;
    #[inline(always)]
    fn read_nil(&mut self) -> Result<()> {
        match self._read_byte() {
            Ok(0xc0) => Ok(()),
            Ok(_)    => Err(_invalid_input("Invalid nil opcode")),
            Err(e)   => Err(e)
        }
    }

    #[inline(always)]
    fn read_u64(&mut self) -> Result<u64> { self._read_unsigned() }

    #[inline(always)]
    fn read_uint(&mut self) -> Result<usize> {
        match num::traits::cast(try!(self._read_unsigned())) {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside usize"))
        }
    }

    #[inline(always)]
    fn read_u32(&mut self) -> Result<u32> {
        match try!(self._read_unsigned()).to_u32() {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside u32"))
        }
    }

    #[inline(always)]
    fn read_u16(&mut self) -> Result<u16> {
        match try!(self._read_unsigned()).to_u16() {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside u16"))
        }
    }

    #[inline(always)]
    fn read_u8(&mut self) -> Result<u8> {
        match try!(self._read_unsigned()).to_u8() {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside u8"))
        }
    }

    #[inline(always)]
    fn read_i64(&mut self) -> Result<i64> {
        self._read_signed()
    }

    #[inline(always)]
    fn read_int(&mut self) -> Result<isize> {
        match num::traits::cast(try!(self._read_signed())) {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside isize"))
        }
    }

    #[inline(always)]
    fn read_i32(&mut self) -> Result<i32> {
        match try!(self._read_signed()).to_i32() {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside i32"))
        }
    }

    #[inline(always)]
    fn read_i16(&mut self) -> Result<i16> {
        match try!(self._read_signed()).to_i16() {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside i16"))
        }
    }

    #[inline(always)]
    fn read_i8(&mut self) -> Result<i8> {
        match try!(self._read_signed()).to_i8() {
            Some(i) => Ok(i),
            None    => Err(_invalid_input("value does not fit inside i8"))
        }
    }

    #[inline(always)]
    fn read_bool(&mut self) -> Result<bool> {
        match try!(self._read_byte()) {
            0xc2 => Ok(false),
            0xc3 => Ok(true),
            _    => Err(_invalid_input("invalid bool"))
        }
    }

    #[inline(always)]
    fn read_f64(&mut self) -> Result<f64> {
        match try!(self._read_byte()) {
            0xcb => read_double(&mut self.rd),
            _    => Err(_invalid_input("invalid f64"))
        }
    }

    #[inline(always)]
    fn read_f32(&mut self) -> Result<f32> {
        match try!(self._read_byte()) {
            0xca => read_float(&mut self.rd),
            _    => Err(_invalid_input("invalid f32"))
        }
    }

    // XXX: Optimize
    #[inline(always)]
    fn read_char(&mut self) -> Result<char> {
        let s = try!(self.read_str());
        if s.len() != 1 { return Err(_invalid_input("invalid char")) }
        Ok(s.chars().next().unwrap())
    }

    #[inline(always)]
    fn read_str(&mut self) -> Result<String> {
        let c = try!(self._read_byte());
        match c {
            0xa0 ... 0xbf => self._read_str((c as usize) & 0x1F),
            0xd9         => {
                let l = try!(self.rd.read_u8()) as usize;
                self._read_str(l)
            },
            0xda         => {
                let l = try!(self.rd.read_u16::<BigEndian>()) as usize;
                self._read_str(l)
            },
            0xdb         => {
                let l = try!(self.rd.read_u32::<BigEndian>()) as usize;
                self._read_str(l)
            },
            _            => Err(_invalid_input("Invalid string"))
        }
    }

    fn read_enum<T,F>(&mut self, _name: &str, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        f(self)
    }

    fn read_enum_variant<T,F>(&mut self, names: &[&str], mut f: F) -> Result<T>
    where F: FnMut(&mut Decoder<R>, usize) -> Result<T> {
        let idx = try!(self.read_seq(|d, _len| {
            let name = try!(d.read_str());
            match names.iter().position(|n| name == *n) {
                Some(idx) => Ok(idx),
                None => { Err(_invalid_input("unknown variant")) },
            }
        }));

        f(self, idx)
    }
    fn read_enum_variant_arg<T,F>(&mut self, _idx: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        f(self)
    }

    #[inline(always)]
    fn read_seq<T,F>(&mut self, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>, usize) -> Result<T> {
        let len = try!(self._read_vec_len());
        f(self, len)
    }

    #[inline(always)]
    fn read_seq_elt<T,F>(&mut self, _idx: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        f(self)
    }

    #[inline(always)]
    fn read_struct<T,F>(&mut self, _name: &str, len: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        // XXX: Why are we using a map length here?
        if len != try!(self._read_map_len()) {
            Err(_invalid_input("invalid length for struct"))
        } else {
            f(self)
        }
    }

    #[inline(always)]
    fn read_struct_field<T,F>(&mut self, _name: &str, _idx: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        f(self)
    }

    fn read_option<T,F>(&mut self, mut f: F) -> Result<T>
    where F: FnMut(&mut Decoder<R>, bool) -> Result<T> {
        match try!(self._peek_byte()) {
            0xc0 => { self._read_byte(); f(self, false) }, // consume the nil byte from packed format
            _    => { f(self, true) },
        }
    }

    fn read_map<T,F>(&mut self, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>, usize) -> Result<T> {
        let len = try!(self._read_map_len());
        f(self, len)
    }

    fn read_map_elt_key<T,F>(&mut self, _idx: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> { f(self) }

    fn read_map_elt_val<T,F>(&mut self, _idx: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> { f(self) }


    fn read_enum_struct_variant<T,F>(&mut self,
                                     names: &[&str],
                                     f: F) -> Result<T>
    where F: FnMut(&mut Decoder<R>, usize) -> Result<T> {
            self.read_enum_variant(names, f)
    }

    fn read_enum_struct_variant_field<T,F>(&mut self,
                                           _name: &str,
                                           idx: usize,
                                           f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        self.read_enum_variant_arg(idx, f)
    }

    fn read_tuple<T,F>(&mut self, exp_len: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        let len = try!(self._read_vec_len());
        if exp_len == len {
            f(self)
        } else {
            panic!("Wrong tuple length") // XXX
        }
    }

    fn read_tuple_arg<T,F>(&mut self, idx: usize, f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        self.read_seq_elt(idx, f)
    }

    fn read_tuple_struct<T,F>(&mut self,
                            _name: &str, len: usize,
                            f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        self.read_tuple(len, f)
    }

    fn read_tuple_struct_arg<T,F>(&mut self,
                                idx: usize,
                                f: F) -> Result<T>
    where F: FnOnce(&mut Decoder<R>) -> Result<T> {
        self.read_tuple_arg(idx, f)
    }

    fn error(&mut self, _err: &str) -> byteorder::Error {
        byteorder::Error::from(std::io::Error::new(ErrorKind::InvalidInput, "ApplicationError"))
    }
}

#[cfg(todo)]
impl serialize::Decodable for Value {
    fn decode<D, R: Read>(s: &mut D) -> Result<Self, D::Error>
        where D: Decoder<R> {
        s.decode_value()
    }
}


/// A structure for implementing serialization to Msgpack.
pub struct Encoder<'a> {
    wr: &'a mut (Write + 'a)
}

impl<'a> Encoder<'a> {
    /// Creates a new Msgpack encoder whose output will be written to the writer
    /// specified.
    pub fn new(wr: &'a mut Write) -> Encoder<'a> {
        Encoder { wr: wr }
    }

    pub fn to_msgpack<T: Encodable>(t: &T) -> Result<Vec<u8>> {
        let mut m = Vec::new();
        {
            let mut encoder = Encoder::new(&mut m as &mut Write);
            try!(t.encode(&mut encoder));
        }
        Ok(m)
    }

    /// Emits the most efficient representation of the given unsigned integer
    fn _emit_unsigned(&mut self, v: u64) -> Result<()> {
        if v <= 127 {
            try!(self.wr.write_u8(v as u8));
        }
        else if v <= std::u8::MAX as u64 {
            try!(self.wr.write_u8(0xcc));
            try!(self.wr.write_u8(v as u8));
        }
        else if v <= std::u16::MAX as u64 {
            try!(self.wr.write_u8(0xcd));
            try!(self.wr.write_u16::<BigEndian>(v as u16));
        }
        else if v <= std::u32::MAX as u64 {
            try!(self.wr.write_u8(0xce));
            try!(self.wr.write_u32::<BigEndian>(v as u32));
        }
        else {
            try!(self.wr.write_u8(0xcf));
            try!(self.wr.write_u64::<BigEndian>(v));
        }

        Ok(())
    }

    /// Emits the most efficient representation of the given signed integer
    fn _emit_signed(&mut self, v: i64) -> Result<()> {
        if v >= std::i8::MIN as i64 && v <= std::i8::MAX as i64 {
            let v = v as i8;
            if (v as u8) & 0xe0 != 0xe0 {
                try!(self.wr.write_u8(0xd0));
            }
            try!(self.wr.write_u8(v as u8));
        }
        else if v >= std::i16::MIN as i64 && v <= std::i16::MAX as i64 {
            let v = v as i16;
            try!(self.wr.write_u8(0xd1));
            try!(self.wr.write_i16::<BigEndian>(v));
        }
        else if v >= std::i32::MIN as i64 && v <= std::i32::MAX as i64 {
            let v = v as i32;
            try!(self.wr.write_u8(0xd2));
            try!(self.wr.write_i32::<BigEndian>(v));
        }
        else {
            try!(self.wr.write_u8(0xd3));
            try!(self.wr.write_i64::<BigEndian>(v));
        }

        Ok(())
    }

    #[inline(always)]
    fn _emit_len(&mut self, len: usize, (op1, sz1): (u8, usize), (op2, sz2): (u8, usize), op3: u8, op4: u8) -> Result<()> {
        if len < sz1 {
            try!(self.wr.write_u8(op1));
        } else if len < sz2 {
            try!(self.wr.write_u8(op2));
            try!(self.wr.write_u8(len as u8));
        } else if len <= std::u16::MAX as usize {
            try!(self.wr.write_u8(op3));
            try!(self.wr.write_u16::<BigEndian>(len as u16));
        } else {
            assert!(len <= std::u32::MAX as usize); // XXX
            try!(self.wr.write_u8(op4));
            try!(self.wr.write_u32::<BigEndian>(len as u32));
        }

        Ok(())
    }

    fn _emit_str_len(&mut self, len: usize) -> Result<()> {
        self._emit_len(len, (0xa0_u8 | (len & 31) as u8, 32),
        (0xd9, 256),
        0xda,
        0xdb)
    }

    fn _emit_bin_len(&mut self, len: usize) -> Result<()> {
        self._emit_len(len, (0x00, 0),
        (0xc4, 256),
        0xc5,
        0xc6)
    }


    fn _emit_array_len(&mut self, len: usize) -> Result<()> {
        self._emit_len(len, (0x90_u8 | (len & 15) as u8, 16),
        (0x00, 0),
        0xdc,
        0xdd)
    }

    fn _emit_map_len(&mut self, len: usize) -> Result<()> {
        self._emit_len(len, (0x80_u8 | (len & 15) as u8, 16),
        (0x00, 0),
        0xde,
        0xdf)
    }
}

impl<'a> serialize::Encoder for Encoder<'a> {
    type Error = byteorder::Error;
    fn emit_nil(&mut self) -> Result<()> { self.wr.write_u8(0xc0) }

    #[inline(always)]
    fn emit_uint(&mut self, v: usize) -> Result<()> { self._emit_unsigned(v as u64) }
    #[inline(always)]
    fn emit_u64(&mut self, v: u64) -> Result<()>   { self._emit_unsigned(v as u64) }
    #[inline(always)]
    fn emit_u32(&mut self, v: u32) -> Result<()>   { self._emit_unsigned(v as u64) }
    #[inline(always)]
    fn emit_u16(&mut self, v: u16) -> Result<()>   { self._emit_unsigned(v as u64) }
    #[inline(always)]
    fn emit_u8(&mut self, v: u8) -> Result<()>     { self._emit_unsigned(v as u64) }

    #[inline(always)]
    fn emit_int(&mut self, v: isize) -> Result<()>  { self._emit_signed(v as i64) }
    #[inline(always)]
    fn emit_i64(&mut self, v: i64) -> Result<()>  { self._emit_signed(v as i64) }
    #[inline(always)]
    fn emit_i32(&mut self, v: i32) -> Result<()>  { self._emit_signed(v as i64) }
    #[inline(always)]
    fn emit_i16(&mut self, v: i16) -> Result<()>  { self._emit_signed(v as i64) }
    #[inline(always)]
    fn emit_i8(&mut self,  v: i8) -> Result<()>   { self._emit_signed(v as i64) }

    fn emit_f64(&mut self, v: f64) -> Result<()> {
        try!(self.wr.write_u8(0xcb));
        unsafe { self.wr.write_u64::<BigEndian>(mem::transmute(v)) }
    }

    fn emit_f32(&mut self, v: f32) -> Result<()> {
        try!(self.wr.write_u8(0xca));
        unsafe { self.wr.write_u32::<BigEndian>(mem::transmute(v)) }
    }

    fn emit_bool(&mut self, v: bool) -> Result<()> {
        if v {
            self.wr.write_u8(0xc3)
        } else {
            self.wr.write_u8(0xc2)
        }
    }

    fn emit_char(&mut self, v: char)  -> Result<()> {
        let mut s = String::with_capacity(1);
        s.push(v);
        self.emit_str(s.as_ref())
    }

    fn emit_str(&mut self, v: &str) -> Result<()> {
        try!(self._emit_str_len(v.len()));
        match self.wr.write_all(v.as_bytes()) {
            Ok(_) => Ok(()),
            Err(why) => Err(Error::from(why))
        }
    }

    fn emit_enum<F>(&mut self, _name: &str, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        f(self)
    }

    fn emit_enum_variant<F>(&mut self, name: &str, _id: usize, cnt: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_seq(cnt + 1, |d| { d.emit_str(name) });
        f(self)
    }

    fn emit_enum_variant_arg<F>(&mut self, _idx: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        f(self)
    }

    fn emit_enum_struct_variant<F>(&mut self, name: &str, id: usize, cnt: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_enum_variant(name, id, cnt, f)
    }

    fn emit_enum_struct_variant_field<F>(&mut self, _name: &str, idx: usize, f: F)  -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_enum_variant_arg(idx, f)
    }

    // TODO: Option, to enable different ways to write out structs
    //       For example, to emit structs as maps/vectors.
    // XXX: Correct to use _emit_map_len here?
    fn emit_struct<F>(&mut self, _name: &str, len: usize, f: F)  -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        try!(self._emit_map_len(len));
        f(self)
    }

    fn emit_struct_field<F>(&mut self, _name: &str, _idx: usize, f: F)  -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        f(self)
    }

    fn emit_tuple<F>(&mut self, len: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_seq(len, f)
    }

    fn emit_tuple_arg<F>(&mut self, idx: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_seq_elt(idx, f)
    }

    fn emit_tuple_struct<F>(&mut self,
                         _name: &str,
                         len: usize,
                         f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_seq(len, f)
    }

    fn emit_tuple_struct_arg<F>(&mut self, idx: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        self.emit_seq_elt(idx, f)
    }

    fn emit_option<F>(&mut self, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> { f(self) }

    fn emit_option_none(&mut self) -> Result<()>  { self.emit_nil() }

    fn emit_option_some<F>(&mut self, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> { f(self) }

    fn emit_seq<F>(&mut self, len: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        try!(self._emit_array_len(len));
        f(self)
    }

    fn emit_seq_elt<F>(&mut self, _idx: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        f(self)
    }

    fn emit_map<F>(&mut self, len: usize, f: F) -> Result<()>
     where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        try!(self._emit_map_len(len));
        f(self)
    }

    fn emit_map_elt_key<F>(&mut self, _idx: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        f(self)
    }

    fn emit_map_elt_val<F>(&mut self, _idx: usize, f: F) -> Result<()>
    where F: FnOnce(&mut Encoder<'a>) -> Result<()> {
        f(self)
    }
}

#[cfg(todo)]
impl<E: serialize::Encoder<S>, S> serialize::Encodable<E, S> for Value {
    fn encode(&self, e: &mut E) -> Result<(), S> {
        match *self {
            Value::Nil => e.emit_nil(),
            Value::Boolean(b) => e.emit_bool(b),
            Value::Integer(i) => e.emit_i64(i),
            Value::Unsigned(u) => e.emit_u64(u),
            Value::Float(f) => e.emit_f32(f),
            Value::Double(d) => e.emit_f64(d),
            Value::Array(ref ary) => {
                e.emit_seq(ary.len(), |e2| {
                    for (i, elt) in ary.iter().enumerate() {
                        try!(e2.emit_seq_elt(i, |e3| { elt.encode(e3) }));
                    }
                    Ok(())
                })
            }
            Value::Map(ref map) => {
                e.emit_map(map.len(), |e2| {
                    for (i, &(ref key, ref val)) in map.iter().enumerate() {
                        try!(e2.emit_map_elt_key(i, |e3| { key.encode(e3) }));
                        try!(e2.emit_map_elt_val(i, |e3| { val.encode(e3) }));
                    }
                    Ok(())
                })
            }
            Value::Str(ref str) => e.emit_str(from_utf8(str.as_ref()).unwrap()), // XXX
            Value::Binary(_) => panic!(), // XXX
            Value::Extended(_, _) => panic!() // XXX
        }

    }
}


pub fn from_msgpack<'a, T: Decodable>(bytes: &'a [u8]) -> Result<T> {
    let rd = BufReader::new(bytes);
    let mut decoder = Decoder::new(rd);
    Decodable::decode(&mut decoder)
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use super::{Encoder, from_msgpack};
    use serialize::Encodable;

    macro_rules! assert_msgpack_circular(
        ($ty:ty, $inp:expr) => (
            {
                let bytes = Encoder::to_msgpack(&$inp).unwrap();
                let value: $ty = from_msgpack(bytes.as_ref()).unwrap();
                assert_eq!($inp, value)
            }
        );
    );

    #[test]
    fn test_circular_str() {
      assert_msgpack_circular!(String, "".to_string());
      assert_msgpack_circular!(String, "a".to_string());
      assert_msgpack_circular!(String, "abcdef".to_string());
    }

    #[test]
    fn test_circular_isize() {
      assert_msgpack_circular!(isize, 123isize);
      assert_msgpack_circular!(isize, -123isize);
    }

    #[test]
    fn test_circular_float() {
      assert_msgpack_circular!(f32, -1243.111 as f32);
    }

    #[test]
    fn test_circular_bool() {
      assert_msgpack_circular!(bool, true);
      assert_msgpack_circular!(bool, false);
    }

    #[test]
    fn test_circular_list() {
      assert_msgpack_circular!(Vec<isize>, vec![1,2,3]);
    }

    #[test]
    fn test_circular_map() {
      let mut v = HashMap::new();
      v.insert(1isize, 2isize);
      v.insert(3isize, 4isize);
      assert_msgpack_circular!(HashMap<isize, isize>, v);
    }

    #[test]
    fn test_circular_option() {
      let v: Option<isize> = Some(1);
      assert_msgpack_circular!(Option<isize>, v);

      let v: Option<isize> = None;
      assert_msgpack_circular!(Option<isize>, v);
    }

    #[test]
    fn test_circular_embedded_option() {
        let v: (Option<isize>, Option<isize>) = (None, Some(1));
        assert_msgpack_circular!((Option<isize>, Option<isize>), v);

        let v: (Option<isize>, Option<isize>) = (Some(1), Some(1));
        assert_msgpack_circular!((Option<isize>, Option<isize>), v);
    }

    #[test]
    fn test_circular_char() {
      assert_msgpack_circular!(char, 'a');
    }

    #[derive(Encodable,Decodable,PartialEq,Debug)]
    struct S {
      f: u8,
      g: u16,
      i: String,
      a: Vec<u32>,
      c: HashMap<u32, u32>
    }

    #[test]
    fn test_circular_struct() {
      let mut c = HashMap::new();
      c.insert(1u32, 2u32);
      c.insert(2u32, 3u32);

      let s1 = S { f: 1u8, g: 2u16, i: "foo".to_string(), a: vec![], c: c.clone() };
      let s2 = S { f: 5u8, g: 1u16, i: "bar".to_string(), a: vec![1,2,3], c: c.clone() };
      let s = vec![s1, s2];

      assert_msgpack_circular!(Vec<S>, s);
    }

    #[test]
    fn test_circular_str_lengths() {
        fn from_char(mut n: usize, c: char) -> String {
            let mut s = String::new();
            while n > 0 {
                s.push(c);
                n -= 1;
            }
            s
        }
        assert_msgpack_circular!(String, from_char(4, 'a'));
        assert_msgpack_circular!(String, from_char(32, 'a'));
        assert_msgpack_circular!(String, from_char(256, 'a'));
        assert_msgpack_circular!(String, from_char(0x10000, 'a'));
    }

    #[derive(Encodable, Decodable, PartialEq, Debug)]
    enum Animal {
        Dog,
        Frog(String, usize),
    }

    #[test]
    fn test_circular_enum() {
        assert_msgpack_circular!(Animal, Animal::Dog);
        assert_msgpack_circular!(Animal, Animal::Frog("Henry".to_string(), 349));
    }
}
