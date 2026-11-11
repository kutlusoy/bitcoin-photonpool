use sha2::{Digest, Sha256};

pub fn sha256d(data: &[u8]) -> [u8; 32] {
    Sha256::digest(Sha256::digest(data)).into()
}

pub fn put_varint(out: &mut Vec<u8>, n: u64) {
    match n {
        0..=0xfc => out.push(n as u8),
        0xfd..=0xffff => {
            out.push(0xfd);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(0xfe);
            out.extend_from_slice(&(n as u32).to_le_bytes());
        }
        _ => {
            out.push(0xff);
            out.extend_from_slice(&n.to_le_bytes());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxOutput {
    pub value: u64,
    pub script: Vec<u8>,
}

/// Bytes of the extranonce inside the coinbase scriptSig: extranonce1 (per connection) then extranonce2 (rolled by the miner).
pub const EN1_BYTES: usize = 4;
pub const EN2_BYTES: usize = 4;
pub const EN_BYTES: usize = EN1_BYTES + EN2_BYTES;

/// Minimal script push of the block height (BIP34).
fn height_push(height: u32) -> Vec<u8> {
    if height == 0 {
        return vec![0x00];
    }
    if height <= 16 {
        return vec![0x50 + height as u8];
    }
    let mut v = height.to_le_bytes().to_vec();
    while v.last() == Some(&0) {
        v.pop();
    }
    if v.last().map_or(false, |b| b & 0x80 != 0) {
        v.push(0);
    }
    let mut out = vec![v.len() as u8];
    out.extend(v);
    out
}

/// Coinbase transaction split around the extranonce, in the non-witness serialisation that defines the txid.
pub struct CoinbaseParts {
    pub coinb1: Vec<u8>,
    pub coinb2: Vec<u8>,
}

impl CoinbaseParts {
    pub fn new(height: u32, tag: &[u8], outputs: &[TxOutput]) -> Self {
        let mut sig = height_push(height);
        if !tag.is_empty() {
            sig.push(tag.len() as u8);
            sig.extend_from_slice(tag);
        }
        let sig_len = sig.len() + EN_BYTES;
        assert!((2..=100).contains(&sig_len), "coinbase scriptSig length");

        let mut c1 = Vec::with_capacity(64);
        c1.extend_from_slice(&2i32.to_le_bytes());
        put_varint(&mut c1, 1);
        c1.extend_from_slice(&[0u8; 32]);
        c1.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        put_varint(&mut c1, sig_len as u64);
        c1.extend_from_slice(&sig);

        let mut c2 = Vec::with_capacity(128);
        c2.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        put_varint(&mut c2, outputs.len() as u64);
        for o in outputs {
            c2.extend_from_slice(&o.value.to_le_bytes());
            put_varint(&mut c2, o.script.len() as u64);
            c2.extend_from_slice(&o.script);
        }
        c2.extend_from_slice(&0u32.to_le_bytes());
        Self { coinb1: c1, coinb2: c2 }
    }

    pub fn assemble(&self, en1: &[u8; EN1_BYTES], en2: &[u8; EN2_BYTES]) -> Vec<u8> {
        let mut cb = Vec::with_capacity(self.coinb1.len() + EN_BYTES + self.coinb2.len());
        cb.extend_from_slice(&self.coinb1);
        cb.extend_from_slice(en1);
        cb.extend_from_slice(en2);
        cb.extend_from_slice(&self.coinb2);
        cb
    }
}

/// Segwit serialisation of a non-witness coinbase: marker, flag and the 32 zero byte witness reserved value.
pub fn coinbase_with_witness(nowit: &[u8]) -> Vec<u8> {
    let body = &nowit[4..nowit.len() - 4];
    let mut w = Vec::with_capacity(nowit.len() + 40);
    w.extend_from_slice(&nowit[..4]);
    w.extend_from_slice(&[0x00, 0x01]);
    w.extend_from_slice(body);
    w.extend_from_slice(&[0x01, 0x20]);
    w.extend_from_slice(&[0u8; 32]);
    w.extend_from_slice(&nowit[nowit.len() - 4..]);
    w
}

fn hash_pair(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(a);
    buf[32..].copy_from_slice(b);
    sha256d(&buf)
}

/// Merkle branch for the coinbase (index 0); `txids` are the other transactions in internal byte order.
pub fn merkle_branch(txids: &[[u8; 32]]) -> Vec<[u8; 32]> {
    let mut level: Vec<[u8; 32]> = std::iter::once([0u8; 32]).chain(txids.iter().copied()).collect();
    let mut branch = Vec::new();
    while level.len() > 1 {
        branch.push(level[1]);
        if level.len() % 2 == 1 {
            level.push(*level.last().unwrap());
        }
        let mut next = vec![[0u8; 32]];
        for pair in level[2..].chunks(2) {
            next.push(hash_pair(&pair[0], &pair[1]));
        }
        level = next;
    }
    branch
}

pub fn merkle_root(coinbase_txid: &[u8; 32], branch: &[[u8; 32]]) -> [u8; 32] {
    branch.iter().fold(*coinbase_txid, |acc, s| hash_pair(&acc, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_root(mut leaves: Vec<[u8; 32]>) -> [u8; 32] {
        while leaves.len() > 1 {
            if leaves.len() % 2 == 1 {
                leaves.push(*leaves.last().unwrap());
            }
            leaves = leaves.chunks(2).map(|p| hash_pair(&p[0], &p[1])).collect();
        }
        leaves[0]
    }

    #[test]
    fn branch_matches_full_merkle_tree() {
        for n in 0..40usize {
            let cb = sha256d(b"coinbase");
            let others: Vec<[u8; 32]> = (0..n).map(|i| sha256d(&(i as u64).to_le_bytes())).collect();
            let mut all = vec![cb];
            all.extend(others.iter().copied());
            assert_eq!(merkle_root(&cb, &merkle_branch(&others)), full_root(all), "n={n}");
        }
    }

    #[test]
    fn varint_boundaries() {
        for (n, len) in [(0u64, 1), (0xfc, 1), (0xfd, 3), (0xffff, 3), (0x1_0000, 5), (0xffff_ffff, 5), (0x1_0000_0000, 9)] {
            let mut v = Vec::new();
            put_varint(&mut v, n);
            assert_eq!(v.len(), len);
        }
    }

    #[test]
    fn height_pushes_follow_cscript() {
        assert_eq!(height_push(1), vec![0x51]);
        assert_eq!(height_push(16), vec![0x60]);
        assert_eq!(height_push(17), vec![0x01, 0x11]);
        assert_eq!(height_push(128), vec![0x02, 0x80, 0x00]);
        assert_eq!(height_push(500_000), vec![0x03, 0x20, 0xa1, 0x07]);
        assert_eq!(height_push(8_388_608), vec![0x04, 0x00, 0x00, 0x80, 0x00]);
    }

    #[test]
    fn genesis_style_coinbase_roundtrip() {
        let outs = vec![TxOutput { value: 50 * 100_000_000, script: vec![0x51] }];
        let parts = CoinbaseParts::new(100, b"/t/", &outs);
        let cb = parts.assemble(&[1, 2, 3, 4], &[5, 6, 7, 8]);
        let w = coinbase_with_witness(&cb);
        assert_eq!(w.len(), cb.len() + 2 + 34);
        assert_eq!(&w[4..6], &[0, 1]);
        assert_eq!(&cb[cb.len() - 4..], &[0, 0, 0, 0]);
        let at = parts.coinb1.len();
        assert_eq!(&cb[at..at + 8], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
