//! ObscuredInt decryption (pure, no IL2CPP).

/// Decrypt an ObscuredInt from its raw 8-byte representation.
/// Layout: bytes [0..4] = cryptoKey (i32 LE), bytes [4..8] = hiddenValue (i32 LE).
/// Result: hiddenValue ^ cryptoKey.
pub fn decrypt_obscured_int_raw(buf: &[u8; 8]) -> i32 {
    let key = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let val = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    val ^ key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_obscured_int_basic() {
        // key=42, hiddenValue=42^100=78 → decrypted=78^42=100
        let key: i32 = 42;
        let plaintext: i32 = 100;
        let hidden = plaintext ^ key;
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&key.to_le_bytes());
        buf[4..8].copy_from_slice(&hidden.to_le_bytes());
        assert_eq!(decrypt_obscured_int_raw(&buf), 100);
    }

    #[test]
    fn decrypt_obscured_int_zero_key() {
        let mut buf = [0u8; 8];
        buf[4..8].copy_from_slice(&999i32.to_le_bytes());
        assert_eq!(decrypt_obscured_int_raw(&buf), 999);
    }

    #[test]
    fn decrypt_obscured_int_negative() {
        let key: i32 = 0x1234_5678;
        let plaintext: i32 = -50;
        let hidden = plaintext ^ key;
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&key.to_le_bytes());
        buf[4..8].copy_from_slice(&hidden.to_le_bytes());
        assert_eq!(decrypt_obscured_int_raw(&buf), -50);
    }

    #[test]
    fn decrypt_obscured_int_roundtrip_all_bits() {
        // Verify XOR is its own inverse
        for &(key, plain) in &[(0xFF_FF_FF_FFu32 as i32, 0), (1, i32::MAX), (i32::MIN, i32::MIN)] {
            let hidden = plain ^ key;
            let mut buf = [0u8; 8];
            buf[0..4].copy_from_slice(&key.to_le_bytes());
            buf[4..8].copy_from_slice(&hidden.to_le_bytes());
            assert_eq!(decrypt_obscured_int_raw(&buf), plain);
        }
    }
}
