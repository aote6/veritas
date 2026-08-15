//! SHA-256 implementation, strictly following FIPS 180-4
//! ("Secure Hash Standard (SHS)", NIST, August 2015), Section 6.2.
//!
//! This is a from-scratch, dependency-free implementation using only
//! `std`. It performs no SIMD or other micro-optimizations; the goal
//! is clarity and auditability against the FIPS 180-4 specification.
//!
//! No `unsafe` code is used anywhere in this module.

/// Initial hash values H0..H7, as specified in FIPS 180-4, Section 5.3.3.
///
/// These are the first 32 bits of the fractional parts of the square
/// roots of the first eight prime numbers (2, 3, 5, 7, 11, 13, 17, 19).
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
    0x5be0cd19,
];

/// Round constants K0..K63, as specified in FIPS 180-4, Section 4.2.2.
///
/// These are the first 32 bits of the fractional parts of the cube
/// roots of the first sixty-four prime numbers (2 .. 311).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// The size, in bytes, of a single SHA-256 message block (512 bits).
const BLOCK_SIZE: usize = 64;

/// Incremental SHA-256 hasher.
///
/// Construct with [`Sha256::new`], feed data with any number of calls
/// to [`Sha256::update`], and obtain the final 32-byte digest with
/// [`Sha256::finalize`]. Feeding data via several `update` calls
/// always produces the same digest as feeding all of the data in a
/// single `update` call.
pub struct Sha256 {
    /// Current hash state (H0..H7), updated after each full block.
    state: [u32; 8],
    /// Partial block buffer holding bytes not yet processed because
    /// they do not yet fill a complete 64-byte block.
    buffer: [u8; BLOCK_SIZE],
    /// Number of valid bytes currently stored in `buffer` (0..=63).
    buffer_len: usize,
    /// Total number of message bytes fed into the hasher so far,
    /// across all calls to `update`. This is accumulated using `u64`
    /// arithmetic (message length, in bytes; the FIPS 180-4 length
    /// field is a 64-bit *bit* count, derived from this by `* 8`).
    total_len: u64,
}

impl Sha256 {
    /// Creates a new SHA-256 hasher with the standard FIPS 180-4
    /// initial hash values and empty internal state.
    pub fn new() -> Sha256 {
        Sha256 {
            state: H0,
            buffer: [0u8; BLOCK_SIZE],
            buffer_len: 0,
            total_len: 0,
        }
    }

    /// Feeds additional message bytes into the hasher.
    ///
    /// May be called any number of times with arbitrary chunk sizes
    /// (including zero-length slices); the result is identical to
    /// calling `update` once with all the data concatenated.
    pub fn update(&mut self, data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);

        let mut offset = 0usize;

        if self.buffer_len > 0 {
            let needed = BLOCK_SIZE - self.buffer_len;
            let available = data.len();
            let take = if available < needed { available } else { needed };

            self.buffer[self.buffer_len..self.buffer_len + take]
                .copy_from_slice(&data[..take]);
            self.buffer_len += take;
            offset += take;

            if self.buffer_len == BLOCK_SIZE {
                let block = self.buffer;
                Self::process_block(&mut self.state, &block);
                self.buffer_len = 0;
            } else {
                return;
            }
        }

        let remaining = &data[offset..];
        let full_blocks = remaining.len() / BLOCK_SIZE;
        for i in 0..full_blocks {
            let start = i * BLOCK_SIZE;
            let block: &[u8; BLOCK_SIZE] =
                remaining[start..start + BLOCK_SIZE].try_into().unwrap();
            Self::process_block(&mut self.state, block);
        }

        let leftover_start = full_blocks * BLOCK_SIZE;
        let leftover = &remaining[leftover_start..];
        self.buffer[..leftover.len()].copy_from_slice(leftover);
        self.buffer_len = leftover.len();
    }

    /// Consumes the hasher, applies FIPS 180-4 padding to the
    /// remaining buffered data, processes the final block(s), and
    /// returns the resulting 32-byte big-endian SHA-256 digest.
    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);

        let mut pad_buf = self.buffer;
        let mut pad_len = self.buffer_len;
        pad_buf[pad_len] = 0x80;
        pad_len += 1;

        if pad_len <= BLOCK_SIZE - 8 {
            for b in pad_buf.iter_mut().skip(pad_len).take(BLOCK_SIZE - 8 - pad_len) {
                *b = 0;
            }
            pad_buf[BLOCK_SIZE - 8..].copy_from_slice(&bit_len.to_be_bytes());
            Self::process_block(&mut self.state, &pad_buf);
        } else {
            for b in pad_buf.iter_mut().skip(pad_len) {
                *b = 0;
            }
            Self::process_block(&mut self.state, &pad_buf);

            let mut final_block = [0u8; BLOCK_SIZE];
            final_block[BLOCK_SIZE - 8..].copy_from_slice(&bit_len.to_be_bytes());
            Self::process_block(&mut self.state, &final_block);
        }

        let mut digest = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    /// Processes a single 512-bit (64-byte) message block.
    fn process_block(state: &mut [u32; 8], block: &[u8; BLOCK_SIZE]) {
        let mut w = [0u32; 64];

        for t in 0..16 {
            let i = t * 4;
            w[t] = u32::from_be_bytes([block[i], block[i + 1], block[i + 2], block[i + 3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16]
                .wrapping_add(s0)
                .wrapping_add(w[t - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for t in 0..64 {
            let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(big_s1)
                .wrapping_add(ch)
                .wrapping_add(K[t])
                .wrapping_add(w[t]);
            let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = big_s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256::new()
    }
}

/// Computes the SHA-256 digest of `data` in a single call.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(digest: &[u8; 32]) -> String {
        let mut s = String::with_capacity(64);
        for byte in digest {
            s.push_str(&format!("{:02x}", byte));
        }
        s
    }

    #[test]
    fn empty_string() {
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(hex(&sha256(b"")), expected);
    }

    #[test]
    fn abc() {
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(hex(&sha256(b"abc")), expected);
    }

    #[test]
    fn quick_brown_fox() {
        let expected = "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592";
        assert_eq!(
            hex(&sha256(b"The quick brown fox jumps over the lazy dog")),
            expected
        );
    }

    #[test]
    fn one_million_a() {
        let expected = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
        let data = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn boundary_55_bytes() {
        let expected = "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318";
        let data = vec![b'a'; 55];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn boundary_56_bytes() {
        let expected = "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a";
        let data = vec![b'a'; 56];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn boundary_57_bytes() {
        let expected = "f13b2d724659eb3bf47f2dd6af1accc87b81f09f59f2b75e5c0bed6589dfe8c6";
        let data = vec![b'a'; 57];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn boundary_63_bytes() {
        let expected = "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34";
        let data = vec![b'a'; 63];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn boundary_64_bytes() {
        let expected = "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb";
        let data = vec![b'a'; 64];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn boundary_65_bytes() {
        let expected = "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0";
        let data = vec![b'a'; 65];
        assert_eq!(hex(&sha256(&data)), expected);
    }

    #[test]
    fn incremental_matches_single_call_various_chunkings() {
        let data = vec![b'a'; 1000];
        let expected = sha256(&data);

        let mut h1 = Sha256::new();
        for byte in &data {
            h1.update(std::slice::from_ref(byte));
        }
        assert_eq!(h1.finalize(), expected);

        let mut h2 = Sha256::new();
        for chunk in data.chunks(7) {
            h2.update(chunk);
        }
        assert_eq!(h2.finalize(), expected);

        let boundary_data = vec![b'b'; 200];
        let expected_boundary = sha256(&boundary_data);
        let mut h3 = Sha256::new();
        h3.update(&boundary_data[..54]);
        h3.update(&boundary_data[54..55]);
        h3.update(&boundary_data[55..56]);
        h3.update(&boundary_data[56..57]);
        h3.update(&boundary_data[57..]);
        assert_eq!(h3.finalize(), expected_boundary);

        let mut h4 = Sha256::new();
        h4.update(b"");
        h4.update(b"abc");
        h4.update(b"");
        assert_eq!(
            hex(&h4.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn empty_update_is_noop() {
        let mut h = Sha256::new();
        h.update(b"");
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(hex(&h.finalize()), expected);
    }
}
