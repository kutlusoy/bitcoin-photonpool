use crate::tx::sha256d;
use anyhow::{anyhow, bail, Result};

const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Testnet,
    Regtest,
}

impl Network {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "mainnet" => Ok(Self::Mainnet),
            "testnet" => Ok(Self::Testnet),
            "regtest" => Ok(Self::Regtest),
            _ => bail!("unknown network {s:?} (mainnet, testnet or regtest)"),
        }
    }

    /// Value of `chain` in getblockchaininfo.
    pub fn chain(self) -> &'static [&'static str] {
        match self {
            Self::Mainnet => &["main"],
            Self::Testnet => &["test", "testnet4"],
            Self::Regtest => &["regtest"],
        }
    }

    fn hrp(self) -> &'static str {
        match self {
            Self::Mainnet => "bc",
            Self::Testnet => "tb",
            Self::Regtest => "bcrt",
        }
    }

    fn p2pkh(self) -> u8 {
        if self == Self::Mainnet { 0x00 } else { 0x6f }
    }

    fn p2sh(self) -> u8 {
        if self == Self::Mainnet { 0x05 } else { 0xc4 }
    }
}

fn base58_decode(s: &str) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    for c in s.bytes() {
        let mut carry = B58.iter().position(|b| *b == c).ok_or_else(|| anyhow!("invalid base58 character"))? as u32;
        for b in out.iter_mut() {
            carry += *b as u32 * 58;
            *b = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            out.push(carry as u8);
            carry >>= 8;
        }
    }
    out.extend(s.bytes().take_while(|c| *c == b'1').map(|_| 0u8));
    out.reverse();
    Ok(out)
}

/// scriptPubKey for a P2PKH, P2SH, P2WPKH, P2WSH or P2TR address of `net`.
pub fn script_for_address(addr: &str, net: Network) -> Result<Vec<u8>> {
    if let Ok((hrp, version, program)) = bech32::segwit::decode(addr) {
        if hrp.as_str() != net.hrp() {
            bail!("address {addr} has prefix {}, expected {}", hrp.as_str(), net.hrp());
        }
        let v = version.to_u8();
        let ok = match v {
            0 => matches!(program.len(), 20 | 32),
            1 => program.len() == 32,
            _ => (2..=40).contains(&program.len()),
        };
        if !ok {
            bail!("address {addr} has an invalid witness program");
        }
        let mut script = vec![if v == 0 { 0x00 } else { 0x50 + v }, program.len() as u8];
        script.extend_from_slice(&program);
        return Ok(script);
    }
    let raw = base58_decode(addr).map_err(|_| anyhow!("{addr} is not a valid address"))?;
    if raw.len() != 25 || sha256d(&raw[..21])[..4] != raw[21..] {
        bail!("{addr} is not a valid address");
    }
    let h = &raw[1..21];
    if raw[0] == net.p2pkh() {
        let mut s = vec![0x76, 0xa9, 0x14];
        s.extend_from_slice(h);
        s.extend_from_slice(&[0x88, 0xac]);
        Ok(s)
    } else if raw[0] == net.p2sh() {
        let mut s = vec![0xa9, 0x14];
        s.extend_from_slice(h);
        s.push(0x87);
        Ok(s)
    } else {
        bail!("address {addr} does not belong to this network")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_mainnet_addresses() {
        let p2pkh = script_for_address("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2", Network::Mainnet).unwrap();
        assert_eq!(hex::encode(p2pkh), "76a91477bff20c60e522dfaa3350c39b030a5d004e839a88ac");
        let p2sh = script_for_address("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", Network::Mainnet).unwrap();
        assert_eq!(hex::encode(p2sh), "a914b472a266d0bd89c13706a4132ccfb16f7c3b9fcb87");
        let wpkh = script_for_address("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", Network::Mainnet).unwrap();
        assert_eq!(hex::encode(wpkh), "0014751e76e8199196d454941c45d1b3a323f1433bd6");
        let tr = script_for_address("bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0", Network::Mainnet).unwrap();
        assert_eq!(hex::encode(tr), "512079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798");
    }

    #[test]
    fn rejects_wrong_network_and_bad_checksum() {
        assert!(script_for_address("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", Network::Testnet).is_err());
        assert!(script_for_address("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN3", Network::Mainnet).is_err());
        assert!(script_for_address("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2", Network::Regtest).is_err());
        assert!(script_for_address("garbage", Network::Mainnet).is_err());
    }
}
