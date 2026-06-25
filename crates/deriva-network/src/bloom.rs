pub struct BloomFilter {
    bits: Vec<u8>,
    num_bits: usize,
    num_hashes: usize,
}

impl BloomFilter {
    pub fn new(num_bits: usize, num_hashes: usize) -> Self {
        let bytes = num_bits.div_ceil(8);
        Self { bits: vec![0u8; bytes], num_bits, num_hashes }
    }

    pub fn insert(&mut self, data: &[u8]) {
        for seed in 0..self.num_hashes as u64 {
            let idx = self.hash(data, seed);
            self.bits[idx / 8] |= 1 << (idx % 8);
        }
    }

    pub fn contains(&self, data: &[u8]) -> bool {
        (0..self.num_hashes as u64).all(|seed| {
            let idx = self.hash(data, seed);
            self.bits[idx / 8] & (1 << (idx % 8)) != 0
        })
    }

    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bits
    }

    pub fn from_bytes(data: &[u8], num_hashes: usize) -> Self {
        let num_bits = data.len() * 8;
        Self { bits: data.to_vec(), num_bits, num_hashes }
    }

    fn hash(&self, data: &[u8], seed: u64) -> usize {
        let mut input = Vec::with_capacity(data.len() + 8);
        input.extend_from_slice(data);
        input.extend_from_slice(&seed.to_le_bytes());
        let h = blake3::hash(&input);
        let bytes = h.as_bytes();
        let val = u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]);
        (val as usize) % self.num_bits
    }
}
