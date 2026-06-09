use ratatui::buffer::Buffer;

/// Convert a ratatui buffer to a plain text string for snapshot testing.
/// Strips trailing whitespace per line.
pub fn buffer_to_string(buf: &Buffer) -> String {
    let mut result = String::new();
    for y in buf.area.y..buf.area.bottom() {
        let mut line = String::new();
        for x in buf.area.x..buf.area.right() {
            let cell = &buf[(x, y)];
            line.push_str(cell.symbol());
        }
        result.push_str(line.trim_end());
        result.push('\n');
    }
    result
}
