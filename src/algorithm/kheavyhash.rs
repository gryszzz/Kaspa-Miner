use super::matrix::Matrix;
use kaspa_hashes::{Hash, PowHash};

pub type Target = [u64; 4];

/// Full Kaspa PoW calculation for one nonce candidate.
#[inline]
pub fn compute(hasher: &PowHash, matrix: &Matrix, nonce: u64) -> Hash {
    let hash = hasher.clone().finalize_with_nonce(nonce);
    matrix.heavy_hash(hash)
}

/// Returns true when little-endian `hash` as u256 is <= little-endian `target`.
#[inline]
pub fn meets_target(hash: &Hash, target: &Target) -> bool {
    let pow = hash.to_le_u64();
    for i in (0..4).rev() {
        match pow[i].cmp(&target[i]) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    true
}

/// Pre-built mining state for a single job.
pub struct JobContext {
    pub target: Target,
    pub matrix: Matrix,
    pub hasher: PowHash,
}

impl JobContext {
    pub fn new(pre_pow_hash: [u8; 32], timestamp: u64, target: Target) -> Self {
        let pre_pow_hash = Hash::from_bytes(pre_pow_hash);
        let matrix = Matrix::generate(pre_pow_hash);
        let hasher = PowHash::new(pre_pow_hash, timestamp);
        Self {
            target,
            matrix,
            hasher,
        }
    }

    /// Try a single nonce. Returns Some(nonce) on success.
    #[inline]
    pub fn try_nonce(&self, nonce: u64) -> Option<u64> {
        let hash = compute(&self.hasher, &self.matrix, nonce);
        meets_target(&hash, &self.target).then_some(nonce)
    }
}
