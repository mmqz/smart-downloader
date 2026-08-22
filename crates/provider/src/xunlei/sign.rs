//! 迅雷云盘签名算法（captcha_sign / device_sign），纯函数。

use md5::{Digest, Md5};

/// 手写 hex 编码（避免引入 hex crate）。
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// captcha_sign 的 10 个盐（alist 开源，MIT）。
const ALGORITHMS: [&str; 10] = [
    "9uJNVj/wLmdwKrJaVj/omlQ",
    "Oz64Lp0GigmChHMf/6TNfxx7O9PyopcczMsnf",
    "Eb+L7Ce+Ej48u",
    "jKY0",
    "ASr0zCl6v8W4aidjPK5KHd1Lq3t+vBFf41dqv5+fnOd",
    "wQlozdg6r1qxh0eRmt3QgNXOvSZO6q/GXK",
    "gmirk+ciAvIgA/cxUUCema47jr/YToixTT+Q6O",
    "5IiCoM9B1/788ntB",
    "P07JH0h6qoM6TSUAK2aL9T5s2QBVeY9JWvalf",
    "+oK0AN",
];

const CLIENT_ID: &str = "Xp6vsxz_7IYVw2BB";
const CLIENT_VERSION: &str = "8.31.0.9726";
const PACKAGE_NAME: &str = "com.xunlei.downloadprovider";

/// 计算 captcha_sign：
///   s = ClientID + ClientVersion + PackageName + DeviceID + timestamp
///   for salt in ALGORITHMS: s = md5(s + salt)
///   返回 "1." + s
pub fn captcha_sign(device_id: &str, timestamp_millis: &str) -> String {
    let mut s = format!(
        "{}{}{}{}{}",
        CLIENT_ID, CLIENT_VERSION, PACKAGE_NAME, device_id, timestamp_millis
    );
    for salt in ALGORITHMS {
        let mut h = Md5::new();
        h.update(s.as_bytes());
        h.update(salt.as_bytes());
        s = to_hex(&h.finalize());
    }
    format!("1.{}", s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encodes_correctly() {
        assert_eq!(to_hex(&[0x00, 0xff, 0x0a]), "00ff0a");
    }

    #[test]
    fn captcha_sign_starts_with_1_dot() {
        let sign = captcha_sign("device123", "1700000000000");
        assert!(sign.starts_with("1."));
    }

    #[test]
    fn captcha_sign_is_32_hex_after_prefix() {
        let sign = captcha_sign("device123", "1700000000000");
        let hex_part = sign.strip_prefix("1.").unwrap();
        assert_eq!(hex_part.len(), 32);
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn captcha_sign_is_deterministic() {
        let a = captcha_sign("dev", "1000");
        let b = captcha_sign("dev", "1000");
        assert_eq!(a, b);
    }

    #[test]
    fn captcha_sign_changes_with_timestamp() {
        let a = captcha_sign("dev", "1000");
        let b = captcha_sign("dev", "1001");
        assert_ne!(a, b);
    }
}
