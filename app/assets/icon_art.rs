/// Parses assets/icon.txt: "X r g b" palette lines, then the pixel rows ('.' = transparent).
/// Shared by build.rs (exe/installer .ico) and main.rs (window icon).
#[allow(dead_code)]
fn icon_pixels(text: &str) -> Vec<Vec<[u8; 4]>> {
    let mut palette = std::collections::HashMap::new();
    let mut rows = Vec::new();
    for line in text.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 4 && parts[0].chars().count() == 1 {
            let v: Vec<u8> = parts[1..].iter().map(|p| p.parse().unwrap_or(0)).collect();
            palette.insert(parts[0].chars().next().unwrap_or('.'), [v[0], v[1], v[2], 255]);
        } else {
            rows.push(line.chars().map(|c| palette.get(&c).copied().unwrap_or([0, 0, 0, 0])).collect());
        }
    }
    rows
}
