//! Deterministic source text for the benches: Rust-like lines of varied length and indentation,
//! with an occasional non-ASCII comment (so both the monospace fast path and the galley path run).

#![allow(dead_code)]

/// `lines` lines of code (about 38 bytes per line on average).
pub fn code(lines: usize) -> String {
    let mut s = String::with_capacity(lines * 40);
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    for i in 0..lines {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let indent = (x % 4) as usize * 4;
        s.extend(std::iter::repeat_n(' ', indent));
        match x % 9 {
            0 => s.push_str(&format!("fn item_{i}(a: u32, b: &str) -> Option<u32> {{")),
            1 => s.push_str(&format!("let value_{i} = compute(a, b, {});", x % 1000)),
            2 => s.push('}'),
            3 => s.push_str(&format!("// note {i}: grüße, naïve café — ok")),
            4 => s.push_str(&format!("if value > {} {{ return None; }}", x % 77)),
            5 => {}
            6 => s.push_str(&format!(
                "map.insert(\"key_{i}\".to_string(), vec![1, 2, 3]);"
            )),
            7 => s.push_str(&format!("self.total += self.items[{}].weight * 2;", x % 50)),
            _ => s.push_str(&format!("match state {{ State::A => {i}, _ => 0 }}")),
        }
        s.push('\n');
    }
    s
}

/// `lines` lines of code with one line of `long_len` bytes in the middle.
pub fn with_long_line(lines: usize, long_len: usize) -> String {
    let mut s = code(lines / 2);
    let unit = "let a = b + c; ";
    let mut long = String::with_capacity(long_len);
    while long.len() < long_len {
        long.push_str(unit);
    }
    long.truncate(long_len);
    s.push_str(&long);
    s.push('\n');
    s.push_str(&code(lines - lines / 2 - 1));
    s
}
