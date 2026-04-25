use super::xoshiro::XoShiRo256PlusPlus;
use kaspa_hashes::{Hash, KHeavyHash};

pub const N: usize = 64;

/// 64x64 nibble matrix generated from a job's pre_pow_hash.
/// Re-generated each time the pool sends new work.
#[derive(Clone)]
pub struct Matrix(pub Box<[[u16; N]; N]>);

impl Matrix {
    /// Generate from pre_pow_hash. Regenerates if any row is all-zero.
    pub fn generate(seed: Hash) -> Self {
        let mut rng = XoShiRo256PlusPlus::new(seed);
        loop {
            let mut data = Box::new([[0u16; N]; N]);
            for row in data.iter_mut() {
                for j in (0..N).step_by(16) {
                    let val = rng.u64();
                    for shift in 0..16 {
                        row[j + shift] = ((val >> (4 * shift)) & 0x0f) as u16;
                    }
                }
            }
            let matrix = Matrix(data);
            if matrix.compute_rank() == 64 {
                return matrix;
            }
        }
    }

    /// Matrix x nibble-vector multiply, xor, then final KHeavyHash.
    #[inline]
    pub fn heavy_hash(&self, hash: Hash) -> Hash {
        let bytes = hash.as_bytes();
        let mut vec = [0u8; N];
        for (i, element) in bytes.iter().enumerate() {
            vec[2 * i] = element >> 4;
            vec[2 * i + 1] = element & 0x0f;
        }

        let mut product = [0u8; 32];
        for i in 0..32 {
            let mut sum1 = 0u16;
            let mut sum2 = 0u16;
            for (j, &elem) in vec.iter().enumerate() {
                sum1 += self.0[2 * i][j] * elem as u16;
                sum2 += self.0[2 * i + 1][j] * elem as u16;
            }
            product[i] = ((sum1 >> 10) << 4) as u8 | (sum2 >> 10) as u8;
            product[i] ^= bytes[i];
        }

        KHeavyHash::hash(Hash::from_bytes(product))
    }

    pub fn compute_rank(&self) -> usize {
        const EPS: f64 = 1e-9;
        let mut mat = [[0.0f64; N]; N];
        for (i, row) in mat.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = f64::from(self.0[i][j]);
            }
        }

        let mut rank = 0;
        let mut row_selected = [false; N];
        for i in 0..N {
            let mut j = 0;
            while j < N {
                if !row_selected[j] && mat[j][i].abs() > EPS {
                    break;
                }
                j += 1;
            }
            if j != N {
                rank += 1;
                row_selected[j] = true;
                for p in (i + 1)..N {
                    mat[j][p] /= mat[j][i];
                }
                for k in 0..N {
                    if k != j && mat[k][i].abs() > EPS {
                        for p in (i + 1)..N {
                            mat[k][p] -= mat[j][p] * mat[k][i];
                        }
                    }
                }
            }
        }
        rank
    }
}
