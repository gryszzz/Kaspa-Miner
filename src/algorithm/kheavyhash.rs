use sha3::{Digest, Sha3_256};

use super::matrix::{Matrix, N};

/// Full kHeavyHash computation for one nonce candidate.
///
/// Algorithm:
///  1. hash1 = SHA3-256(pre_pow_hash ‖ timestamp_le ‖ nonce_le)
///  2. Expand hash1 into 64 nibbles (input vector)
///  3. product = matrix × input_vector  (wrapping u16, result normalised to nibbles)
///  4. Pack product nibbles back to 32 bytes, XOR with hash1
///  5. Return Blake3(xored)
#[inline]
pub fn compute(pre_pow_hash: &[u8; 32], matrix: &Matrix, timestamp: u64, nonce: u64) -> [u8; 32] {
    // --- step 1 ---
    let mut h = Sha3_256::new();
    h.update(pre_pow_hash);
    h.update(timestamp.to_le_bytes());
    h.update(nonce.to_le_bytes());
    let hash1: [u8; 32] = h.finalize().into();

    // --- step 2: each byte → two nibbles ---
    let mut vec = [0u16; N];
    for i in 0..32 {
        vec[2 * i]     = (hash1[i] >> 4) as u16;
        vec[2 * i + 1] = (hash1[i] & 0xF) as u16;
    }

    // --- step 3: matrix multiply ---
    let product = matrix.multiply(&vec);

    // --- step 4: pack + XOR ---
    let mut xored = [0u8; 32];
    for i in 0..32 {
        let packed = ((product[2 * i] as u8) << 4) | (product[2 * i + 1] as u8);
        xored[i] = packed ^ hash1[i];
    }

    // --- step 5: final hash ---
    *blake3::hash(&xored).as_bytes()
}

/// Returns true when `hash` (big-endian u256) ≤ `target`.
#[inline]
pub fn meets_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    for i in 0..32 {
        match hash[i].cmp(&target[i]) {
            std::cmp::Ordering::Less    => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal   => {}
        }
    }
    true
}

/// Pre-built mining state for a single job.
pub struct JobContext {
    pub pre_pow_hash: [u8; 32],
    pub timestamp:    u64,
    pub target:       [u8; 32],
    pub matrix:       Matrix,
}

impl JobContext {
    pub fn new(pre_pow_hash: [u8; 32], timestamp: u64, target: [u8; 32]) -> Self {
        let matrix = Matrix::generate(&pre_pow_hash);
        Self { pre_pow_hash, timestamp, target, matrix }
    }

    /// Try a single nonce. Returns Some(nonce) on success.
    #[inline]
    pub fn try_nonce(&self, nonce: u64) -> Option<u64> {
        let hash = compute(&self.pre_pow_hash, &self.matrix, self.timestamp, nonce);
        meets_target(&hash, &self.target).then_some(nonce)
    }
}
