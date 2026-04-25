use super::xoshiro::XoShiRo256PlusPlus;

pub const N: usize = 64;

/// 64×64 u16 matrix generated from a job's pre_pow_hash.
/// Re-generated each time the pool sends new work.
#[derive(Clone)]
pub struct Matrix(pub Box<[[u16; N]; N]>);

impl Matrix {
    /// Generate from pre_pow_hash. Regenerates if any row is all-zero.
    pub fn generate(seed: &[u8; 32]) -> Self {
        let mut rng = XoShiRo256PlusPlus::new(seed);
        loop {
            let mut data = Box::new([[0u16; N]; N]);
            for row in data.iter_mut() {
                for cell in row.iter_mut() {
                    *cell = rng.next_u16();
                }
            }
            if data.iter().all(|row| row.iter().any(|&v| v != 0)) {
                return Matrix(data);
            }
        }
    }

    /// Matrix × nibble-vector multiply with wrapping u16 arithmetic.
    /// Input: 64 nibble values (0–15 each).
    /// Output: 64 nibble values (bits [13:10] of each row dot-product).
    #[inline]
    pub fn multiply(&self, vec: &[u16; N]) -> [u16; N] {
        let mut out = [0u16; N];
        for i in 0..N {
            let mut sum: u16 = 0;
            for j in 0..N {
                sum = sum.wrapping_add(self.0[i][j].wrapping_mul(vec[j]));
            }
            out[i] = (sum >> 10) & 0xF;
        }
        out
    }
}
