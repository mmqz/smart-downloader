//! 迅雷 GCID / CID 文件内容哈希（xunlei-lixian 公开算法）。

use sha1::{Digest, Sha1};

/// 手写 hex 编码。
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// GCID 的 piece 大小：0x40000 起，file_size/piece_size > 0x200 时翻倍，上限 0x200000。
fn calc_block_size(file_size: u64) -> u64 {
    let mut psize: u64 = 0x40000;
    while file_size / psize > 0x200 && psize < 0x200000 {
        psize <<= 1;
    }
    psize
}

/// GCID = SHA1( SHA1(piece1) || SHA1(piece2) || ... ) 的 hex。
pub fn gcid(data: &[u8]) -> String {
    let mut hash1 = Sha1::new();
    let psize = calc_block_size(data.len() as u64) as usize;
    for chunk in data.chunks(psize) {
        hash1.update(Sha1::digest(chunk));
    }
    to_hex(&hash1.finalize())
}

/// CID (=DCID)：文件 <60KB → SHA1(全文)；否则 SHA1(头20KB || 1/3处20KB || 尾20KB)。
pub fn cid(data: &[u8]) -> String {
    if data.len() < 60 * 1024 {
        return to_hex(&Sha1::digest(data));
    }
    let head = &data[0..20 * 1024];
    let mid_start = data.len() / 3;
    let mid = &data[mid_start..mid_start + 20 * 1024];
    let tail = &data[data.len() - 20 * 1024..];
    let mut h = Sha1::new();
    h.update(head);
    h.update(mid);
    h.update(tail);
    to_hex(&h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_small_file_is_256k() {
        assert_eq!(calc_block_size(1024 * 1024), 0x40000);
    }

    #[test]
    fn block_size_caps_at_2m() {
        assert_eq!(calc_block_size(100 * 1024 * 1024 * 1024), 0x200000);
    }

    #[test]
    fn gcid_is_40_hex() {
        let data = vec![0xAAu8; 1024 * 1024];
        let gcid = gcid(&data);
        assert_eq!(gcid.len(), 40);
        assert!(gcid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn gcid_is_deterministic() {
        let data = vec![0xBBu8; 4096];
        assert_eq!(gcid(&data), gcid(&data));
    }

    #[test]
    fn gcid_empty_data() {
        assert_eq!(
            gcid(&[]),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn cid_small_file_is_full_sha1() {
        let data = vec![0x11u8; 1000];
        assert_eq!(cid(&data), to_hex(&Sha1::digest(&data)));
    }

    #[test]
    fn cid_large_file_samples_three_regions() {
        let data = vec![0x22u8; 100 * 1024];
        let c = cid(&data);
        assert_eq!(c.len(), 40);

        let head = &data[0..20 * 1024];
        let mid_start = data.len() / 3;
        let mid = &data[mid_start..mid_start + 20 * 1024];
        let tail = &data[data.len() - 20 * 1024..];
        let mut h = Sha1::new();
        h.update(head);
        h.update(mid);
        h.update(tail);
        assert_eq!(c, to_hex(&h.finalize()));
    }
}
