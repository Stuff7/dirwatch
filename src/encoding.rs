const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(input: &[u8]) -> String {
  let mut output = String::new();
  let mut i = 0;

  while i < input.len() {
    let mut buffer = [0u8; 3];
    let chunk_size = std::cmp::min(input.len() - i, 3);
    buffer[..chunk_size].copy_from_slice(&input[i..i + chunk_size]);

    let n = (buffer[0] as u32) << 16 | (buffer[1] as u32) << 8 | (buffer[2] as u32);

    output.push(BASE64_TABLE[((n >> 18) & 0x3F) as usize] as char);
    output.push(BASE64_TABLE[((n >> 12) & 0x3F) as usize] as char);

    if chunk_size > 1 {
      output.push(BASE64_TABLE[((n >> 6) & 0x3F) as usize] as char);
    }
    else {
      output.push(b'=' as char);
    }

    if chunk_size > 2 {
      output.push(BASE64_TABLE[(n & 0x3F) as usize] as char);
    }
    else {
      output.push(b'=' as char);
    }

    i += 3;
  }

  output
}

const BLOCK_SIZE: usize = 64;

pub struct Sha1 {
  data: [u8; 64],
  length: usize,
  h: [u32; 5],
}

impl Sha1 {
  pub fn new() -> Self {
    Sha1 {
      data: [0; 64],
      length: 0,
      h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
    }
  }

  pub fn hash(data: &[u8]) -> [u8; 20] {
    let mut hasher = Self::new();
    hasher.update(data).finalize()
  }

  pub fn update(&mut self, input: &[u8]) -> &mut Self {
    let mut i = 0;
    let mut len = input.len();
    while len > 0 {
      let space = BLOCK_SIZE - (self.length % BLOCK_SIZE);
      let amount = len.min(space);
      self.data[self.length % BLOCK_SIZE..self.length % BLOCK_SIZE + amount].copy_from_slice(&input[i..i + amount]);
      self.length += amount;
      i += amount;
      len -= amount;

      if self.length % BLOCK_SIZE == 0 {
        self.process_block();
      }
    }
    self
  }

  pub fn finalize(&mut self) -> [u8; 20] {
    let bit_len = self.length * 8;
    let pad_len = if self.length % BLOCK_SIZE < 56 {
      56 - self.length % BLOCK_SIZE
    }
    else {
      120 - self.length % BLOCK_SIZE
    };

    self.update(&[0x80]);
    self.update(&vec![0; pad_len - 1]);

    self.update(&(bit_len as u64).to_be_bytes());

    let mut result = [0u8; 20];
    for (i, &val) in self.h.iter().enumerate() {
      result[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }

    result
  }

  fn process_block(&mut self) {
    let mut w = [0u32; 80];
    for (i, w) in w.iter_mut().enumerate().take(16) {
      *w = u32::from_be_bytes(self.data[i * 4..(i + 1) * 4].try_into().unwrap());
    }
    for i in 16..80 {
      w[i] = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
      w[i] = w[i].rotate_left(1);
    }

    let (mut a, mut b, mut c, mut d, mut e) = (self.h[0], self.h[1], self.h[2], self.h[3], self.h[4]);

    for (i, w) in w.iter().enumerate() {
      let (f, k) = if i < 20 {
        ((b & c) | ((!b) & d), 0x5A827999)
      }
      else if i < 40 {
        (b ^ c ^ d, 0x6ED9EBA1)
      }
      else if i < 60 {
        ((b & c) | (b & d) | (c & d), 0x8F1BBCDC)
      }
      else {
        (b ^ c ^ d, 0xCA62C1D6)
      };

      let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(*w).wrapping_add(k);
      e = d;
      d = c;
      c = b.rotate_left(30);
      b = a;
      a = temp;
    }

    self.h[0] = self.h[0].wrapping_add(a);
    self.h[1] = self.h[1].wrapping_add(b);
    self.h[2] = self.h[2].wrapping_add(c);
    self.h[3] = self.h[3].wrapping_add(d);
    self.h[4] = self.h[4].wrapping_add(e);
  }
}
